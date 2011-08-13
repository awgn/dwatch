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
#include <signal.h>

#include <iostream>
#include <fstream>
#include <limits>
#include <cstring>
#include <string>
#include <vector>

#include <tuple>
#include <chrono>
#include <functional>
#include <algorithm>
#include <stdexcept>
#include <thread>
#include <unordered_map>
#include <atomic>

extern const char *__progname;

typedef std::pair<size_t, size_t>  range_type;


//////////////// global data /////////////////


const char * const CLEAR = "\E[2J";
const char * const EDOWN = "\E[J";
const char * const HOME  = "\E[H";
const char * const ELINE = "\E[2K";
const char * const BOLD  = "\E[1m";
const char * const RESET = "\E[0m";
const char * const BLUE  = "\E[1;34m";
const char * const RED   = "\E[31m";

int g_seconds = std::numeric_limits<int>::max();

std::function<bool(char c)> g_euristic; 

std::chrono::seconds g_interval(1);

bool g_color = false;

std::string    g_datafile;

std::ofstream  g_data;

typedef void(showpol_t)(std::ostream &, int64_t, bool);

std::function<showpol_t> g_showpol;
std::atomic_int  g_sigpol;
std::atomic_bool g_diffmode(false); 


std::vector< std::function<showpol_t> > g_showvec = 
{
    [](std::ostream &out, int64_t val, bool reset)
    {
        if (val != 0 && g_diffmode)
        {
            out << '[' << (g_color ? BOLD : "") << val << RESET << ']';
        }
    }, 

    [](std::ostream &out, int64_t val, bool reset)
    {
        static int counter = 0;
        if (reset) {
            counter = 0;
            return;
        }
        out << '(' << (g_color ? BOLD : "") << ++counter << RESET << ')';
    },

    /* policy suitable for diffmode */

    [](std::ostream &out, int64_t val, bool reset) 
    {
        auto rate = static_cast<double>(val)/g_interval.count();
        if (rate != 0.0) {
            out << '[';
            if (rate > 1000000000)
                out << (g_color ? BOLD : "") << rate/1000000000 << "G/sec" << RESET; 
            else if (rate > 1000000)
                out << (g_color ? BOLD : "") << rate/1000000 << "M/sec" << RESET; 
            else if (rate > 1000)
                out << (g_color ? BOLD : "") << rate/1000 << "K/sec" << RESET; 
            else 
                out << (g_color ? BOLD : "") << rate << "/sec" << RESET;
            out << ']';
        }
    }
};


//////////////// defaut euristic /////////////////


struct default_euristic
{
    default_euristic(const char *sep)
    : xs(sep)
    {}

    bool operator()(char c) const
    {
        auto issep = [&](char a) 
        {
            for(char x : xs)
            {
                if (a == x)
                    return true;
            }
            return false;
        };

        return isspace(c) || issep(c); 
    }

    std::string xs;
};


void signal_handler(int sig)
{
    switch(sig)
    {
    case SIGQUIT: 
         g_sigpol++;
         break;
    case SIGTSTP:
         g_diffmode.store(g_diffmode.load() ? false : true);
         break;
    case SIGWINCH:
         std::cout << CLEAR;
         break;
    }; 
}


std::vector<range_type>
get_ranges(const char *str)
{
    std::vector<range_type> local_vector;

    enum class state { none, space, sign, digit };
    state local_state = state::space;

    range_type local_point;
    std::string::size_type local_index = 0;

    // parse line...
    //

    for(const char *c = str; *c != '\0'; c++)
    {
        switch(local_state)
        {
        case state::none:
            {
                if (g_euristic(*c))
                    local_state = state::space;
            } break;
        case state::space:
            {       
                if (isdigit(*c)) {
                    local_state = state::digit;
                    local_point.first = local_index;
                } else if (*c == '-' || *c == '+') {
                    local_state = state::sign;
                    local_point.first = local_index;
                }
                else if (!g_euristic(*c)) {
                    local_state = state::none;
                }    
            } break;        
        case state::sign:
            {
                if (isdigit(*c)) {
                    local_state = state::digit;
                } else if (*c == '-' || *c == '+') {
                    local_state = state::sign;
                    local_point.first = local_index;
                }
                else if (!g_euristic(*c)) {
                    local_state = state::none;
                }    
            } break;
        case state::digit:
            {
                if (g_euristic(*c)) {
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
    std::vector<range_type> ret;
    size_t first = 0;

    for(const range_type &x : xs)
    {
        ret.push_back(std::make_pair(first, x.first));
        first = x.second;
    }
    ret.push_back(std::make_pair(first, size));

    ret.erase(std::remove_if(ret.begin(), ret.end(), 
             [](const range_type &r) { return r.first == r.second; }), ret.end());
    return ret;
}


inline bool 
in_range(std::string::size_type i, const std::vector<range_type> &xs)
{
    for(const range_type &x : xs)
    {
        if (i < x.first)
            return false;
        if (i >= x.first && i < x.second)
            return true;
    }
    return false;
}


inline std::vector<int64_t>
get_mutables(const char *str, const std::vector<range_type> &xs)
{
    std::vector<int64_t> ret;
    for(const range_type &x : xs)
    {    
        ret.push_back(stoll(std::string(str + x.first, str + x.second)));
    }
    return ret;
}                 


inline std::vector<std::string>
get_immutables(const char *str, const std::vector<range_type> &xs)
{
    std::vector<std::string> ret;
    for(const range_type &x : complement(xs, strlen(str)))
    {
        ret.push_back(std::string(str + x.first, str + x.second));
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
                  if (!in_range(index++, xs) && !isdigit(c)) 
                      str.push_back(c); 
                  }); 
    str.erase(str.size()-1,1);
    return std::make_pair(std::hash<std::string>()(str),str);
}


void
stream_line(std::ostream &out, const std::vector<std::string> &i, 
            const std::vector<int64_t> &m, const std::vector<int64_t> &d, std::vector<range_type> &xs)
{
    auto it = i.cbegin(), it_e = i.cend();
    auto mt = m.cbegin(), mt_e = m.cend();
    auto dt = d.cbegin(), dt_e = d.cend();

    if (!xs.empty() && xs[0].first == 0) 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( mt != mt_e ) out << (g_color ? BLUE : "") << *mt++ << RESET;
        if ( dt != dt_e ) g_showpol(out, *dt++, /* reset */ false);
        if ( it != it_e ) out << *it++;
    }
    else 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( it != it_e ) out << *it++;
        if ( mt != mt_e ) out << (g_color ? BLUE : "") << *mt++ << RESET;
        if ( dt != dt_e ) g_showpol(out, *dt++, /* reset */ false);
    }
}   


void 
show_line(size_t n, const char *line)
{
    static std::unordered_map<size_t, std::tuple<uint32_t, std::vector<range_type>, std::vector<int64_t> >> dmap;

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

        std::cout << ELINE << line << '\n';
    }
    else 
    {
        decltype(values) diff(values.size());
        std::transform(values.begin(), values.end(),
                       std::get<2>(it->second).begin(), diff.begin(), std::minus<int64_t>());

        // dump datafile if open...
        auto & xs= g_diffmode ? diff : values;
        if (g_data.is_open()) {
            for(int64_t x : xs)
            {
                g_data << x << '\t';
            }
        }

#ifdef DEBUG
        std::cout << "+"   << c0 << c1 << c2 << c3 << 
                     " h:" << std::hex << h.first << std::dec << 
                     "'"   << h.second << "' -> ";
#endif
        // dump the line...
        std::cout << ELINE;
        stream_line(std::cout, get_immutables(line, ranges), values, xs, ranges);
        std::cout << '\n';
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

    std::cout << CLEAR;

    for(int n=0; n < g_seconds; ++n)
    {
        size_t show_index = (g_sigpol % (g_diffmode ? g_showvec.size() : 2));

        // set the display policy
        //
        
        g_showpol = g_showvec[show_index];


        int status, fds[2];
        if (::pipe(fds) < 0)
            throw std::runtime_error(std::string("pipe: ").append(strerror(errno)));

        pid_t pid = fork();
        if (pid == -1)
            throw std::runtime_error(std::string("fork: ").append(strerror(errno)));

        if (pid == 0) {  
            
            /* child */

            ::close(fds[0]); /* for reading */
            ::close(1);
            ::dup2(fds[1], 1);

            ::execl("/bin/sh", "sh", "-c", command, NULL);
            ::_exit(127);
        }
        else { 
            
            /* parent */

            ::close(fds[1]); /* for writing */

            // display the header: 
            //

            std::cout << HOME << ELINE << "Every " << g_interval.count() << "s: '" << command << "' diff:" <<
                (g_color ? BOLD : "") << (g_diffmode ? "ON " : "OFF ") << RESET <<
                "showmode:" << (g_color ? BOLD : "") << show_index << RESET << " ";
            if (g_data.is_open())
                std::cout << "trace:" << g_datafile;
            std::cout << '\n'; 

            // dump the output
            //

            if (g_data.is_open())
                g_data << n << '\t';
            
            FILE * fp = ::fdopen(fds[0], "r");
            char *line = NULL;  
            size_t nbyte, len = 0, i = 0;
            
            while( (nbyte = ::getline(&line, &len, fp)) != -1 )
            {   
                // replace '\n' with '\0'...
                line[nbyte-1] = '\0';
                show_line(i++,line); 
            }

            // flush the stdout...
            std::cout << EDOWN << std::flush;

            ::free(line);
            ::fclose(fp);
            
            // dump output
            //
            
            if (g_data.is_open())
                g_data << std::endl;

            /* wait for termination */

            while (::waitpid(pid, &status, 0) == -1) {
                if (errno != EINTR) {     
                    status = -1;
                    break;  /* exit loop */
                }
            }
        }

        g_showpol(std::cout, 0, /* reset */ true); 

        std::this_thread::sleep_for(g_interval);
    }

    return 0;
}                   


void usage()
{
    std::cout << "usage: " << __progname << 
        " [-h] [-c|--color] [-i|--interval sec] [-t|--trace trace.out]\n"
        "       [-e|--euristic level] [-d|--diff] [-n sec] command [args...]" << std::endl;
}


int
main(int argc, char *argv[])
try
{
    if (argc < 2) {
        usage();
        return 0;
    }
    
    char **opt = &argv[1];

    // parse command line option...
    //
    
    for ( ; opt != (argv + argc) ; opt++)
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
        if (!std::strcmp(*opt, "-d") || !std::strcmp(*opt, "--diff"))
        {
            g_diffmode.store(true);
            continue;
        }
        if (!std::strcmp(*opt, "-i") || !std::strcmp(*opt, "--interval"))
        {
            g_interval = std::chrono::seconds(atoi(*++opt));
            continue;
        }
        if (!std::strcmp(*opt, "-t") || !std::strcmp(*opt, "--trace"))
        {
            g_datafile.assign(*++opt);
            continue;
        }
        if (!std::strcmp(*opt, "-e") || !std::strcmp(*opt, "--euristic"))
        {
            switch (atoi(*++opt))
            {
            case 0: 
                    g_euristic = default_euristic(",:;()"); 
                    break;
            case 1:
                    g_euristic = default_euristic(".,:;(){}[]="); 
            break;
            default:
                throw std::runtime_error("unknown euristic");
            }
            continue;
        }
        
        break;
    }

    if (opt == (argv + argc))
        throw std::runtime_error("missing argument");
    
    if (!g_euristic)
        g_euristic = default_euristic(",:;()"); 

    if ((signal(SIGQUIT, signal_handler) == SIG_ERR) ||
        (signal(SIGTSTP, signal_handler) == SIG_ERR) ||
        (signal(SIGWINCH, signal_handler) == SIG_ERR) 
       )
        throw std::runtime_error("signal");

    return main_loop(*opt);
}
catch(std::exception &e)
{
    std::cerr << __progname << ": " << e.what() << std::endl;
}
