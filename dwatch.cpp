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
#include <sys/ioctl.h>
#include <unistd.h>

#include <iostream>
#include <fstream>
#include <cstring>
#include <string>
#include <vector>

#include <tuple>
#include <chrono>
#include <functional>
#include <algorithm>
#include <stdexcept>
#include <csignal>
#include <system_error>

#include <thread>
#include <unordered_map>


extern const char *__progname;


typedef std::pair<size_t, size_t>  range_type;


namespace vt100 
{
    const char * const CLEAR = "\E[2J";
    const char * const EDOWN = "\E[J";
    const char * const DOWN  = "\E[1B";
    const char * const HOME  = "\E[H";
    const char * const ELINE = "\E[K";
    const char * const BOLD  = "\E[1m";
    const char * const RESET = "\E[0m";
    const char * const BLUE  = "\E[1;34m";
    const char * const RED   = "\E[31m";

    inline std::pair<unsigned short, unsigned short>
    winsize()
    {
        struct winsize w;
        if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == -1)
            return std::make_pair(0,0);
        return std::make_pair(w.ws_row, w.ws_col);
    }

    template <typename CharT, typename Traits>
    typename std::basic_ostream<CharT, Traits> &
    eline(std::basic_ostream<CharT, Traits> &out, size_t pos, size_t n = 0) 
    {
        out << "\r\E[" << pos << 'C'; 
        if (n == 0)
            return out << ELINE;

        n = std::min(n, winsize().second - pos);
        for(size_t i = 0; i < n; ++i)
           out.put(' ');
        
        return out << "\r\E[" << pos << 'C'; 
    }
}


typedef void(showpol_t)(std::ostream &, int64_t, bool);

std::function<bool(char c)> g_heuristic; 
std::function<showpol_t>    g_showpol;
int                         g_seconds = std::numeric_limits<int>::max();
size_t                      g_tab;
bool                        g_color;
bool                        g_daemon;
bool                        g_banner = true;
std::string                 g_datafile;
std::ofstream               g_data;
volatile std::sig_atomic_t  g_sigpol;
volatile std::sig_atomic_t  g_diffmode; 
std::chrono::milliseconds   g_interval(1000);


std::vector< std::function<showpol_t> > g_showvec = 
{
    [](std::ostream &out, int64_t, bool reset)
    {
        static int counter = 0;
        if (reset) {
            counter = 0;
            return;
        }
        out << '[' << (g_color ? vt100::BOLD : "") << ++counter << vt100::RESET << ']';
    },
    
    [](std::ostream &out, int64_t val, bool)
    {
        if (val != 0 && g_diffmode)
        {
            out << '(' << (g_color ? vt100::BOLD : "") << val << vt100::RESET << ')';
        }
    }, 

    // policy suitable for diffmode 

    [](std::ostream &out, int64_t val, bool) 
    {
        auto rate = static_cast<long double>(val)*1000/g_interval.count();
        if (rate > 0.0) {
            out << '(';
            if (rate > 1000000000)
                out << (g_color ? vt100::BOLD : "") << rate/1000000000 << "G/sec" << vt100::RESET; 
            else if (rate > 1000000)
                out << (g_color ? vt100::BOLD : "") << rate/1000000 << "M/sec" << vt100::RESET; 
            else if (rate > 1000)
                out << (g_color ? vt100::BOLD : "") << rate/1000 << "K/sec" << vt100::RESET; 
            else 
                out << (g_color ? vt100::BOLD : "") << rate << "/sec" << vt100::RESET;
            out << ')';
        }
    }
};


//////////////// default heuristic /////////////////


struct default_heuristic
{
    default_heuristic(const char *sep)
    : xs(sep)
    {}

    bool operator()(char c) const
    {
        auto issep = [&](char a) -> bool 
        {
            for(auto x : xs)
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
         g_diffmode = (g_diffmode ? 0 : 1);
         break;
    case SIGWINCH:
         std::cout << vt100::CLEAR;
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

    // parse a line...

    for(const char *c = str; *c != '\0'; c++)
    {
        switch(local_state)
        {
        case state::none:
            {
                if (g_heuristic(*c))
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
                else if (!g_heuristic(*c)) {
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
                else if (!g_heuristic(*c)) {
                    local_state = state::none;
                }    
            } break;
        case state::digit:
            {
                if (g_heuristic(*c)) {
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

    ret.reserve(xs.size() + 1);
    for(auto &x : xs)
    {
        ret.push_back(std::make_pair(first, x.first));
        first = x.second;
    }

    ret.push_back(std::make_pair(first, size));

    ret.erase(std::remove_if(std::begin(ret), std::end(ret), 
                [](const range_type &r) { return r.first == r.second; }), std::end(ret));
    return ret;
}


inline bool 
in_range(std::string::size_type i, const std::vector<range_type> &xs)
{
    for(auto &x : xs)
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
    ret.reserve(xs.size());
    for(auto &x : xs)
    {    
        ret.push_back(stoll(std::string(str + x.first, str + x.second)));
    }
    return ret;
}                 


inline std::vector<std::string>
get_immutables(const char *str, const std::vector<range_type> &xs)
{
    std::vector<std::string> ret;
    ret.reserve(xs.size());
    for(auto &x : complement(xs, strlen(str)))
    {
        ret.push_back(std::string(str + x.first, str + x.second));
    };
    return ret;
}                 


std::pair<uint32_t, std::string>
hash_line(const char *s, const std::vector<range_type> &xs)
{
    auto size = strlen(s);

    std::string str;
    str.reserve(size);

    size_t index = 0;

    std::for_each(s, s+size, [&](char c) { 
                  if (!in_range(index++, xs) && !isdigit(c)) 
                      str.push_back(c); 
                  }); 

    if (str.size())
        str.erase(str.size()-1, 1);

    auto h = std::hash<std::string>()(str);

    return std::make_pair(h, std::move(str));
}


template <typename CharT, typename Traits>
std::basic_ostream<CharT, Traits> &
show_line(std::basic_ostream<CharT, Traits> &out,
          const std::vector<std::string> &i, const std::vector<int64_t> &m, 
          const std::vector<int64_t> &d, std::vector<range_type> &xs)
{
    auto it = i.cbegin(), it_e = i.cend();
    auto mt = m.cbegin(), mt_e = m.cend();
    auto dt = d.cbegin(), dt_e = d.cend();

    if (!xs.empty() && xs[0].first == 0) 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( mt != mt_e ) out << (g_color ? vt100::BLUE : "") << *mt++ << vt100::RESET;
        if ( dt != dt_e ) g_showpol(out, *dt++, /* reset */ false);
        if ( it != it_e ) out << *it++;
    }
    else 
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( it != it_e ) out << *it++;
        if ( mt != mt_e ) out << (g_color ? vt100::BLUE : "") << *mt++ << vt100::RESET;
        if ( dt != dt_e ) g_showpol(out, *dt++, /* reset */ false);
    }

    return out;
}   


std::pair< std::vector<int64_t>, std::vector<int64_t> >
process_line(size_t n, size_t col, const char *line)
{
    static std::unordered_map<size_t, std::tuple<uint32_t, std::vector<range_type>, std::vector<int64_t> >> dmap;

    auto ranges = get_ranges(line);
    auto values = get_mutables(line, ranges);
    auto imm    = get_immutables(line, ranges);
    auto h      = hash_line(line, ranges);

    decltype(values) diff(values.size());
    
    auto it = dmap.find(n);
    if (it != std::end(dmap))
    {
        // make sure the transform is safe...
        //
        if (values.size() == std::get<2>(it->second).size())
        {
            std::transform(std::begin(values), std::end(values),
                            std::begin(std::get<2>(it->second)), std::begin(diff), std::minus<int64_t>());
        }
    }
    
    dmap[n] = std::make_tuple(h.first, ranges, values); 
    
    // show this line...
    
    auto &xs = g_diffmode ? diff : values;

    // clear this line, completely or partially
    
    vt100::eline(std::cout, col, g_tab); 

    show_line(std::cout, imm, values, xs, ranges); 

    std::cout << std::endl;

    return std::make_pair(values, diff);
}


int 
main_loop(const std::vector<std::string>& commands)
{
    // open data file...
    
    if (!g_datafile.empty()) {
        g_data.open(g_datafile.c_str());
        if (!g_data.is_open())
            throw std::system_error(errno, std::generic_category(), "ofstream::open");
    }

    std::cout << vt100::CLEAR;

    auto now = std::chrono::system_clock::now();

    for(int n=0; n < g_seconds; ++n)
    {
        size_t show_index = static_cast<size_t>(g_sigpol) % (g_diffmode ? g_showvec.size() : 2);

        // set the display policy
        
        g_showpol = g_showvec[show_index];

        // display the header: 

        std::cout << vt100::HOME << vt100::ELINE;

        if (g_banner)
        {
            std::cout << "Every " << g_interval.count() << "ms: ";  
            std::for_each(std::begin(commands), std::end(commands), [](const std::string &c) {
                          std::cout << "'" << c << "' ";
                          });

            std::cout << "diff:" << (g_color ? vt100::BOLD : "") << (g_diffmode ? "ON " : "OFF ") << vt100::RESET <<
                "showmode:" << (g_color ? vt100::BOLD : "") << show_index << vt100::RESET << " ";

            if (g_data.is_open())
                std::cout << "trace:" << g_datafile;

            std::cout << '\n'; 
        }

        // dump the timestamp on trace output

        if (g_data.is_open())
            g_data << n << '\t';

        // dump output of different commands...
        
        size_t i = 0, j = 0;
        
        for(auto const &command : commands)
        {
            if (g_tab) {
                std::cout << vt100::HOME << vt100::DOWN;
            }

            int status, fds[2];
            if (::pipe(fds) < 0)
                throw std::system_error(errno, std::generic_category(), "pipe");

            pid_t pid = fork();
            if (pid == -1)
                throw std::system_error(errno, std::generic_category(), "fork");

            if (pid == 0) {  
                
                /// child ///

                ::close(fds[0]); // for reading
                ::close(1);
                ::dup2(fds[1], 1);
                ::execl("/bin/sh", "sh", "-c", command.c_str(), nullptr);
                ::_exit(127);
            }
                
            /// parent ///

            ::close(fds[1]); // for writing 

            FILE * fp = ::fdopen(fds[0], "r");
            char *line = nullptr;  
            ssize_t nbyte; size_t len = 0;
            
            while( (nbyte = ::getline(&line, &len, fp)) != -1 )
            {   
                // replace '\n' with '\0'...
                line[nbyte-1] = '\0';

                // process and show this line...
                    
                auto data = process_line(i++, g_tab *j, line); 
                
                // dump to datafile if open...
                
                if (g_data.is_open()) {
                    auto & xs = g_diffmode ? data.second : data.first;
                    for(int64_t x : xs)
                    {
                        g_data << x << '\t';
                    }
                }
            }

            // flush the stdout...
            
            std::cout << vt100::EDOWN << std::flush;

            ::free(line);
            ::fclose(fp);
            
            // wait for termination 

            while (::waitpid(pid, &status, 0) == -1) {
                if (errno != EINTR) {     
                    throw std::system_error(errno, std::generic_category(), "waitpid");
                }
            }

            // std::cout << "exit:" << WIFEXITED(status) << " code:" << WEXITSTATUS(status) << std::endl;

            if (!WIFEXITED(status) || 
                 WEXITSTATUS(status) == 2 ||
                 WEXITSTATUS(status) == 126 ||
                 WEXITSTATUS(status) == 127 )
                throw std::runtime_error(std::string("exec: ") + command + std::string(" : error!"));
        
            j++; 
        }
        
        // dump new-line on data...
            
        if (g_data.is_open())
            g_data << std::endl;
        
        g_showpol(std::cout, 0, /* reset */ true); 

        now += g_interval;

        std::this_thread::sleep_until(now);
    }

    return 0;
}                   


void usage()
{
    std::cout << "usage: " << __progname << 
        " [-h] [-c|--color] [-i|--interval sec] [-x|--no-banner] [-t|--trace trace.out]\n"
        "       [-e|--heuristic level] [-d|--diff] [--tab column] [--daemon] [-n sec] 'command' ['commands'...] " << std::endl;
    _Exit(0);
}


int
main(int argc, char *argv[])
try
{
    if (argc < 2) 
        usage();
    
    char **opt = &argv[1];

    auto is_opt = [](const char *arg, const char *opt, const char *opt2) 
    {
        return std::strcmp(arg, opt) == 0 || std::strcmp(arg, opt2) == 0;
    };


    // parse command line option...
    //

    for ( ; opt != (argv + argc) ; opt++)
    {
        if (is_opt(*opt, "-h", "--help"))
        {
            usage(); return 0;
        }
        if (is_opt(*opt, "-n", ""))
        {
            g_seconds = atoi(*++opt);
            continue;
        }
        if (is_opt(*opt, "-c", "--color"))
        {
            g_color = true;
            continue;
        }
        if (is_opt(*opt, "-d", "--diff"))
        {
            g_diffmode = 1;
            continue;
        }
        if (is_opt(*opt, "-x", "--no-banner"))
        {
            g_banner = false;
            continue;
        }
        if (is_opt(*opt, "-i", "--interval"))
        {
            g_interval = std::chrono::milliseconds(atoi(*++opt));
            continue;
        }
        if (is_opt(*opt, "-t", "--trace"))
        {
            g_datafile.assign(*++opt);
            continue;
        }
        if (is_opt(*opt, "--tab", ""))
        {
            g_tab = strtoul(*++opt, nullptr, 0);
            continue;
        }
        if (is_opt(*opt, "", "--daemon"))
        {
            g_daemon = true;
            continue;
        }
        if (is_opt(*opt, "-e", "--heuristic"))
        {
            switch (atoi(*++opt))
            {
            case 0: 
                    g_heuristic = default_heuristic(",:;()"); 
                    break;
            case 1:
                    g_heuristic = default_heuristic(".,:;(){}[]="); 
            break;
            default:
                throw std::runtime_error("unknown heuristic");
            }
            continue;
        }
        
        break;
    }

    if (opt == (argv + argc))
        throw std::runtime_error("missing argument");
    
    if (!g_heuristic)
        g_heuristic = default_heuristic(",:;()"); 

    if ((signal(SIGQUIT, signal_handler) == SIG_ERR) ||
        (signal(SIGTSTP, signal_handler) == SIG_ERR) ||
        (signal(SIGWINCH, signal_handler) == SIG_ERR) 
       )
        throw std::runtime_error("signal");

    if (g_daemon && g_datafile.empty())
        throw std::runtime_error("--daemon option meaningless without --trace");

    if (g_daemon) daemon(1,0);

    std::vector<std::string> commands(opt, argv+argc);

    return main_loop(commands);
}
catch(std::exception &e)
{
    std::cerr << __progname << ": " << e.what() << std::endl;
}

