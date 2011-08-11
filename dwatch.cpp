 /*
 *  Copyright (c) 2011 Bonelli Nicola <bonelli@antifork.org>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, write to the Free Software
 *  Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA 02111-1307, USA.
 *
 */

#include <sys/types.h>
#include <sys/wait.h>

#include <iostream>
#include <fstream>
#include <chrono>
#include <limits>
#include <cstring>
#include <string>
#include <unordered_map>
#include <algorithm>
#include <stdexcept>
#include <thread>

const char * const CLEAR = "\033[2J\033[1;1H";
const char * const BOLD  = "\E[1m";
const char * const RESET = "\E[0m";
const char * const BLUE  = "\E[1;34m";
const char * const RED   = "\E[31m";

extern const char *__progname;

typedef std::pair<size_t, size_t>  range_type;

//// global options /////

int g_seconds = std::numeric_limits<int>::max();

bool g_color = false;

std::chrono::seconds g_interval(1);

std::string    g_datafile;
std::ofstream  g_data;


std::vector<range_type>
get_ranges(const char *str)
{
    std::vector<range_type> local_vector;

    enum class state { none, space, digit };
    state local_state = state::space;

    range_type local_point;
    std::string::size_type local_index = 0;

    for(const char *c = str; *c != '\0'; c++)
    {
        auto is_sep = [](char c) { 
            return isspace(c) || c == ',' || c == ':' || c == ';' || c == '(' || c == ')'; 
        };

        switch(local_state)
        {
        case state::none:
            {
                if (is_sep(*c))
                    local_state = state::space;
            } break;
        case state::space:
            {       
                if (isdigit(*c)) {
                    local_state = state::digit;
                    local_point.first = local_index;
                } else if (!is_sep(*c)) {
                    local_state = state::none;
                }    
            } break;        
        case state::digit:
            {
                if (is_sep(*c)) {
                    local_point.second = local_index;
                    local_vector.push_back(local_point);
                    local_state = state::space;
                }
                else if (!isdigit(*c)) {
                    local_state = state::none;
                } 
            } break;
        }
        local_index++;
    }

    if (local_state == state::digit)
    {
        local_point.second = local_index;
        local_vector.push_back(local_point);
    }

    return local_vector;
}


std::vector<range_type>
complement(const std::vector<range_type> &xs, size_t size)
{
    std::vector<range_type> is;
    size_t first = 0;

    for(const range_type &ip : xs)
    {
        is.push_back(std::make_pair(first, ip.first));
        first = ip.second;
    }
    is.push_back(std::make_pair(first, size));

    is.erase(std::remove_if(is.begin(), is.end(), 
             [](const range_type &r) { return r.first == r.second; }), is.end());
    return is;
}


inline bool 
in_range(std::string::size_type i, const std::vector<range_type> &xs)
{
    for(const range_type &r : xs)
    {
        if (i < r.first)
            return false;
        if (i >= r.first && i < r.second)
            return true;
    }
    return false;
}


inline std::vector<uint64_t>
get_mutables(const char *str, const std::vector<range_type> &mp)
{
    std::vector<uint64_t> ret;
    for(const range_type &p : mp)
    {    
        ret.push_back(stoi(std::string(str + p.first, str + p.second)));
    }
    return ret;
}                 


inline std::vector<std::string>
get_immutables(const char *str, const std::vector<range_type> &mp)
{
    std::vector<std::string> ret;
    for(const range_type &p: complement(mp, strlen(str)))
    {
        ret.push_back(std::string(str + p.first, str + p.second));
    };
    return ret;
}                 


std::pair<uint32_t, std::string>
hash_line(const char *s, const std::vector<range_type> &xs)
{
    const char *s_end = s + strlen(s);
    std::string str;
    str.reserve(s_end-s);

    size_t index = 0;
    std::for_each(s, s_end, [&](char c) { 
                  if (!in_range(index++, xs)) 
                      str.push_back(isdigit(c) ? '_' : c); 
                  }); 
    str.erase(str.size()-1,1);
    return std::make_pair(std::hash<std::string>()(str),str);
}


void
stream_line(std::ostream &out, const std::vector<std::string> &i, 
            const std::vector<uint64_t> &m, const std::vector<uint64_t> &d, std::vector<range_type> &xs)
{
    auto it = i.cbegin(), it_e = i.cend();
    auto mt = m.cbegin(), mt_e = m.cend();
    auto dt = d.cbegin(), dt_e = d.cend();

    auto print_rate = [&]() {
        auto rate = static_cast<double>(*dt)/g_interval.count();
        if (rate != 0.0)
            out << "[" << (g_color ? BOLD : "") << rate << "/sec" << RESET << "]"; 
        dt++;
    };

    if (!xs.empty() && xs[0].first == 0) 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( mt != mt_e ) out << (g_color ? BLUE : "") << *mt++ << RESET;
        if ( dt != dt_e ) print_rate();
        if ( it != it_e ) out << *it++;
    }
    else 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( it != it_e ) out << *it++;
        if ( mt != mt_e ) out << (g_color ? BLUE : "") << *mt++ << RESET;
        if ( dt != dt_e ) print_rate();
    }
}   


void 
show_line(size_t n, const char *line)
{
    static std::unordered_map<size_t, std::tuple<uint32_t, std::vector<range_type>, std::vector<uint64_t> >> dmap;

    auto ranges = get_ranges(line);
    auto h      = hash_line(line, ranges);
    auto values = get_mutables(line, ranges);
    auto it     = dmap.find(n);

    bool c0 = (it == dmap.end());
    bool c1 = c0 || (ranges.empty());
    bool c2 = c1 || std::get<0>(it->second) != h.first;
    bool c3 = c2 || std::get<1>(it->second).size() != ranges.size();

    if (c3) 
    {
#ifdef DEBUG
        std::cout << "+"   << c0 << c1 << c2 << c3 << 
                     " h:" << std::hex << h.first << std::dec << 
                     "'"   << h.second << "' -> ";
#endif

        std::cout << line;
    }
    else 
    {
        std::vector<uint64_t> diff(values.size());
        std::transform(values.begin(), values.end(),
                       std::get<2>(it->second).begin(), diff.begin(), std::minus<uint64_t>());

        // dump datafile if open...
        if (g_data.is_open())
            std::for_each(diff.begin(), diff.end(), [&](uint64_t d) {
                g_data << static_cast<double>(d)/g_interval.count() << '\t';
            });

#ifdef DEBUG
        std::cout << "+"   << c0 << c1 << c2 << c3 << 
                     " h:" << std::hex << h.first << std::dec << 
                     "'"   << h.second << "' -> ";
#endif
        // dump the line...
        stream_line(std::cout, get_immutables(line, ranges), values, diff, ranges);
    }

    dmap[n] = std::make_tuple(h.first, ranges, values); 
}


int 
main_loop(const char *command)
{
    // open data file...
    if (!g_datafile.empty()) {
        g_data.open(g_datafile.c_str());
        if (!g_data.is_open())
            throw std::runtime_error("ofstream::open");
    }

    for(int n=0; n < g_seconds; ++n)
    {
        std::cout << CLEAR << "Every " << g_interval.count() << "s: '" << command << "' "; 
        if (g_data.is_open())
            std::cout << "\tdata:" << g_datafile << std::endl;

        int status, fds[2];
        if (::pipe(fds) < 0)
            throw std::runtime_error(std::string("pipe: ").append(strerror(errno)));

        pid_t pid = fork();
        if (pid == -1)
            throw std::runtime_error(std::string("fork: ").append(strerror(errno)));

        if (pid == 0) {  /* child */

            ::close(fds[0]); /* for reading */
            ::close(1);
            ::dup2(fds[1], 1);

            ::execl("/bin/sh", "sh", "-c", command, NULL);
            ::_exit(127);
        }
        else { /* parent */

            ::close(fds[1]); /* for writing */

            FILE * fp = ::fdopen(fds[0], "r");
            char *line; size_t len = 0; ssize_t read;

            // dump output 
            if (g_data.is_open())
                g_data << n << '\t';

            size_t i = 0;
            while( (read = ::getline(&line, &len, fp)) != -1 )
            {   
                show_line(i++,line); 
            }

            if (g_data.is_open())
                g_data << std::endl;

            ::free(line);
            ::fclose(fp);

            /* wait for termination */

            while (::waitpid(pid, &status, 0) == -1) {
                if (errno != EINTR) {     
                    status = -1;
                    break;  /* exit loop */
                }
            }
        }
        std::this_thread::sleep_for(g_interval);
    }

    return 0;
}                   

void usage()
{
    std::cout << "usage: " << __progname << " [-h] [-c|--color] [-i|--interval sec] [-d|--data data.out ] [-n sec] command [args...]" << std::endl;
}


int
main(int argc, char *argv[])
{
    if (argc < 2) {
        usage();
        return 0;
    }
    
    char **opt = &argv[1];

    // parse command line option...
    //
    
    for ( ; opt != argv + argc ; opt++)
    {
        if (!std::strcmp(*opt, "-h") || !std::strcmp(*opt, "--help"))
        {
            usage(); return 0;
        }
        if (!std::strcmp(*opt, "-n"))
        {
            g_seconds = atoi(*++opt);
            continue;
        }
        if (!std::strcmp(*opt, "-c") || !std::strcmp(*opt, "--color"))
        {
            g_color = true;
            continue;
        }
        if (!std::strcmp(*opt, "-i") || !std::strcmp(*opt, "--interval"))
        {
            g_interval = std::chrono::seconds(atoi(*++opt));
            continue;
        }
        if (!std::strcmp(*opt, "-d") || !std::strcmp(*opt, "--data"))
        {
            g_datafile.assign(*++opt);
            continue;
        }
        break;
    }

    return main_loop(*opt);
}

