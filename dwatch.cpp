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

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <sched.h>

#include <sys/types.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <unistd.h>

#include <iostream>
#include <fstream>
#include <cstring>
#include <sstream>
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

namespace option
{
    int                         seconds = std::numeric_limits<int>::max();

    size_t                      tab;
    bool                        colors;
    bool                        daemon;
    bool                        drop_zero;
    bool                        banner = true;
    int                         cpu = -1;

    std::string                 datafile;
    std::ofstream               data;

    volatile std::sig_atomic_t  showpol = 1;
    volatile std::sig_atomic_t  diffmode;

    std::chrono::microseconds   nominal_interval(1000000);
    std::chrono::microseconds   interval(1000000);
}


namespace vt100
{
    namespace details
    {
        const char * const CLEAR = "\E[2J";
        const char * const EDOWN = "\E[J";
        const char * const DOWN  = "\E[1B";
        const char * const HOME  = "\E[H";
        const char * const ELINE = "\E[K";
        const char * const BOLD  = "\E[1m";
        const char * const RESET = "\E[0m";
        const char * const BLUE  = "\E[1;34m";
        const char * const GREEN = "\E[1;32m";
        const char * const RED   = "\E[31m";
    }

    inline const char * clear()  { return details::CLEAR; }
    inline const char * edown()  { return details::EDOWN; }
    inline const char * down()   { return details::DOWN; }
    inline const char * home()   { return details::HOME; }
    inline const char * eline()  { return details::ELINE; }
    inline const char * reset()  { return details::RESET; }
    inline const char * bold()   { return option::colors ? details::BOLD  : ""; }
    inline const char * green()  { return option::colors ? details::GREEN : ""; }
    inline const char * blue()   { return option::colors ? details::BLUE  : ""; }
    inline const char * red()    { return option::colors ? details::RED   : ""; }


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
            return out << vt100::eline();

        n = std::min(n, winsize().second - pos);
        for(size_t i = 0; i < n; ++i)
           out.put(' ');

        return out << "\r\E[" << pos << 'C';
    }
}


namespace dwatch
{
    using show_type  = void(std::ostream &, int64_t, int64_t, bool);
    using range_type = std::pair<size_t, size_t>;

    std::function<bool(char c)> heuristic;
    std::function<show_type>    show_function;

    template <typename T>
    std::string pretty(T v, bool bit = false)
    {
        double value = v;
        std::ostringstream out;

        if (bit)
        {
            if (value > 1000000000)
                out << value/1000000000 << "Gbps";
            else if (value > 1000000)
                out << value/1000000 << "Mbps";
            else if (value > 1000)
                out << value/1000 << "Kbps";
            else
                out << value << "bps";
        }
        else
        {
            if (value > 1000000000)
                out << value/1000000000 << "G";
            else if (value > 1000000)
                out << value/1000000 << "M";
            else if (value > 1000)
                out << value/1000 << "K";
            else
                out << value;
        }

        return out.str();
    }

    std::vector<std::function<show_type>> show_alg =
    {
        [](std::ostream &out, int64_t, int64_t, bool rst)
        {
            static int counter = 0;
            if (rst) {
                counter = 0;
                return;
            }
            out << '[' << vt100::bold() << ++counter << vt100::reset() << ']';
        },

        [](std::ostream &out, int64_t val, int64_t, bool rst)
        {
            if (rst) return;

            out << vt100::blue() << val << vt100::reset();
        },

        [](std::ostream &out, int64_t val, int64_t diff, bool rst)
        {
            if (rst) return;

            out << vt100::blue() << val << vt100::reset();
            if (diff != 0)
            {
                out << vt100::bold() << '|' << vt100::red() << diff << vt100::reset();
            }
        },

        [](std::ostream &out, int64_t, int64_t diff, bool rst)
        {
            if (rst) return;
            out << vt100::red() << vt100::bold() << diff << vt100::reset();
        },

        [](std::ostream &out, int64_t, int64_t diff, bool rst)
        {
            if (rst) return;
            auto rate = static_cast<double>(diff*1000000)/option::interval.count();
            out << vt100::red() << vt100::bold() << pretty(rate) << vt100::reset();
        },

        [](std::ostream &out, int64_t val, int64_t diff, bool rst)
        {
            if (rst) return;

            auto rate = static_cast<double>(diff*1000000)/option::interval.count();
            out << vt100::blue() << val << vt100::reset();

            if (rate > 0.0)
            {
                out << vt100::bold() << vt100::red() << '|' << pretty(rate) << vt100::reset();
            }
        }
        ,
        [](std::ostream &out, int64_t val, int64_t diff, bool rst)
        {
            if (rst) return;

            auto rate = static_cast<double>(diff*1000000)/option::interval.count();
            if (rate > 0)
            {
                out << vt100::bold() << vt100::blue() << pretty(rate) << vt100::green() << '|' << pretty(rate*8, true) << vt100::reset();
            }
            else
            {
                out << vt100::blue() << val << vt100::reset();
            }
        }
    };
}


struct heuristic_parser
{
    heuristic_parser()
    : idx_(0)
    , xs_{
            ",:;()[]{}<>'`\"|",
            ".,:;()[]{}<>'`\"|",
         }
    {}

    bool operator()(char c) const
    {
        auto issep = [&](char a) -> bool
        {
            for(auto x : xs_[idx()])
                if (a == x)
                    return true;
            return false;
        };

        return isspace(c) || issep(c);
    }

    void next(size_t n = 1)
    {
        idx_ += n;
    }

    size_t
    idx() const
    {
        return idx_ % xs_.size();
    }

    private:

    size_t  idx_;
    std::vector<std::string> xs_;
};


void signal_handler(int sig)
{
    switch(sig)
    {
    case SIGINT:
         dwatch::heuristic.target<heuristic_parser>()->next();
         break;
    case SIGQUIT:
         option::showpol++;
         break;
    case SIGTSTP:
         option::diffmode = (option::diffmode ? 0 : 1);
         break;
    case SIGWINCH:
         std::cout << vt100::clear();
         break;
    };
}


std::vector<dwatch::range_type>
get_ranges(const char *str)
{
    std::vector<dwatch::range_type> local_vector;

    enum class state { none, space, sign, digit };
    state local_state = state::space;

    dwatch::range_type local_point;
    std::string::size_type local_index = 0;

    // parse a line...

    for(const char *c = str; *c != '\0'; c++)
    {
        switch(local_state)
        {
        case state::none:
            {
                if (dwatch::heuristic(*c))
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
                else if (!dwatch::heuristic(*c)) {
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
                else if (!dwatch::heuristic(*c)) {
                    local_state = state::none;
                }
            } break;
        case state::digit:
            {
                if (dwatch::heuristic(*c)) {
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


std::vector<dwatch::range_type>
complement_ranges(std::vector<dwatch::range_type> const &xs, size_t size)
{
    std::vector<dwatch::range_type> ret;
    size_t first = 0;

    ret.reserve(xs.size() + 1);
    for(auto &x : xs)
    {
        ret.push_back(std::make_pair(first, x.first));
        first = x.second;
    }

    ret.push_back(std::make_pair(first, size));

    ret.erase(std::remove_if(std::begin(ret), std::end(ret),
                [](const dwatch::range_type &r) { return r.first == r.second; }), std::end(ret));
    return ret;
}


inline bool
in_range(std::string::size_type i, std::vector<dwatch::range_type> const &xs)
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
get_mutables(const char *str, std::vector<dwatch::range_type> const &xs)
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
get_immutables(const char *str, std::vector<dwatch::range_type> const &xs)
{
    std::vector<std::string> ret;
    ret.reserve(xs.size());
    for(auto &x : complement_ranges(xs, strlen(str)))
    {
        ret.push_back(std::string(str + x.first, str + x.second));
    };
    return ret;
}


uint32_t
hash_line(const char *s, std::vector<dwatch::range_type> const &xs)
{
    auto size = strlen(s);
    size_t index = 0;

    std::string str;
    str.reserve(size);

    std::for_each(s, s+size, [&](char c)
    {
        if (!in_range(index++, xs) && !isdigit(c))
            str.push_back(c);
    });

    if (str.size())
        str.erase(str.size()-1, 1);

    return std::hash<std::string>()(str);
}


template <typename CharT, typename Traits>
std::basic_ostream<CharT, Traits> &
show_line(std::basic_ostream<CharT, Traits> &out,
          std::vector<std::string>        const &i,
          std::vector<int64_t>            const &m,
          std::vector<int64_t>            const &d,
          std::vector<dwatch::range_type> const &xs)
{
    auto it = i.cbegin(), it_e = i.cend();
    auto mt = m.cbegin(), mt_e = m.cend();
    auto dt = d.cbegin(), dt_e = d.cend();

    if (!xs.empty() && xs[0].first == 0)
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( dt != dt_e ) dwatch::show_function(out, *mt++, *dt++, false);
        if ( it != it_e ) out << *it++;
    }
    else
        for(; (it != it_e) || (mt != mt_e);)
    {
        if ( it != it_e ) out << *it++;
        if ( dt != dt_e ) dwatch::show_function(out, *mt++, *dt++, false);
    }

    return out;
}


template <typename CharT, typename Traits>
std::pair< std::vector<int64_t>, std::vector<int64_t> >
process_line(std::basic_ostream<CharT, Traits> &out, size_t n, size_t col, const char *line)
{
    static std::unordered_map<size_t,
        std::tuple<uint32_t, std::vector<dwatch::range_type>, std::vector<int64_t>>> dmap;

    auto ranges  = get_ranges(line);
    auto strings = get_immutables(line, ranges);
    auto values  = get_mutables(line, ranges);
    auto h       = hash_line(line, ranges);

    decltype(values) delta(values.size());

    auto it = dmap.find(n);
    if (it != std::end(dmap))
    {
        // make sure std::transform is safe...
        //
        if (values.size() == std::get<2>(it->second).size())
        {
            std::transform(std::begin(values), std::end(values),
                            std::begin(std::get<2>(it->second)), std::begin(delta), std::minus<int64_t>());
        }
    }

    dmap[n] = std::make_tuple(h, ranges, values);

    // show this line...
    //

    if (!option::drop_zero || std::any_of(std::begin(values), std::end(values), [](uint64_t v) { return v != 0; }))
    {
        // clear this line either completely or partially

        vt100::eline(out, col, option::tab);

        // show the line...

        show_line(out, strings, values, delta, ranges);

        // put endline...
        //
        out << '\n';
    }

    // return the values and delta

    return std::make_pair(values, delta);
}


int
main_loop(std::vector<std::string> const & commands)
{
    // open data file...

    if (!option::datafile.empty()) {
        option::data.open(option::datafile.c_str());
        if (!option::data.is_open())
            throw std::system_error(errno, std::generic_category(), "ofstream::open");
    }

    std::cout << vt100::clear();

    auto prev = std::chrono::system_clock::now();

    for(int n=0; n < option::seconds; ++n)
    {
        std::ostringstream out;

        size_t show_index = static_cast<size_t>(option::showpol) % dwatch::show_alg.size();

        // set the display policy

        dwatch::show_function = dwatch::show_alg[show_index];

        // display the header:

        out << vt100::home() << vt100::eline();

        // display the banner:

        if (option::banner)
        {
            out << "Every " << option::nominal_interval.count()/1000 << "ms: ";

            for(auto const & c : commands)
                out << "'" << c << "' ";

            out <<  "diff:"      << vt100::bold() << (option::diffmode ? "ON " : "OFF ")                   << vt100::reset() <<
                    "showmode:"  << vt100::bold() << show_index                                            << vt100::reset() << " " <<
                    "heuristic:" << vt100::bold() << dwatch::heuristic.target<heuristic_parser>()->idx()   << vt100::reset() << " " << std::endl;

            if (option::data.is_open())
                out << "trace:" << option::datafile << " ";
        }

        // dump the timestamp on trace output

        if (option::data.is_open())
            option::data << n << '\t';

        // dump output of different commands...

        size_t i = 0, j = 0;

        auto now = std::chrono::system_clock::now();
        option::interval = std::chrono::duration_cast<std::chrono::microseconds>(now - prev);

        // set affinity of the parent
        //

#ifdef __linux__
        cpu_set_t set;
        CPU_ZERO(&set);

        if (option::cpu >= 0) {
            CPU_SET(option::cpu, &set);
            if (sched_setaffinity(getpid(), sizeof(set), &set) == -1)
                throw std::system_error(errno, std::generic_category(), "sched_setaffinity");
        }
#endif

        for(auto const &command : commands)
        {
            if (option::tab) {
                out << vt100::home() << vt100::down();
            }

            int status, fds[2];
            if (::pipe(fds) < 0)
                throw std::system_error(errno, std::generic_category(), "pipe");

            pid_t pid = fork();
            if (pid == -1)
                throw std::system_error(errno, std::generic_category(), "fork");

            if (pid == 0) {

                /// child ///

                if (option::cpu >= 0) {
#ifdef __linux__
                    if (sched_setaffinity(getpid(), sizeof(set), &set) == -1)
                        throw std::system_error(errno, std::generic_category(), "sched_setaffinity");
#endif
                }

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

                auto data = process_line(out, i++, option::tab *j, line);

                // dump to datafile if open...

                if (option::data.is_open()) {
                    auto & xs = option::diffmode ? data.second : data.first;
                    for(int64_t x : xs)
                    {
                        option::data << x << '\t';
                    }
                }
            }

            // flush the stdout...

            out << vt100::edown() << std::flush;

            ::free(line);
            ::fclose(fp);

            // wait for termination

            while (::waitpid(pid, &status, 0) == -1) {
                if (errno != EINTR) {
                    throw std::system_error(errno, std::generic_category(), "waitpid");
                }
            }

            if (!WIFEXITED(status))
                out << "'" << command << "': (!WIFEXITED)" << std::endl;

            else if (WEXITSTATUS(status) != 0)
                out << "'" << command << "': (exit status = " << WEXITSTATUS(status) << ")!" << std::endl;

            j++;
        }

        // dump new-line on data...

        if (option::data.is_open())
            option::data << std::endl;

        dwatch::show_function(out, 0, 0, /* reset */ true);

        std::cout << out.str() << std::flush;

        // sleep for the nominal interval...

        prev = now;

        std::this_thread::sleep_until(now + option::nominal_interval);
    }


    return 0;
}


void usage()
{
    std::cout << "usage: " << __progname <<
        " [-h] [-c|--color] [-C cpu] [-i|--interval msec] [-x|--no-banner] [-t|--trace trace.out]\n"
        "       [-e|-ee|-eee|--heuristic] [-d|-dd|-ddd|--diff] [-z|--drop-zero] [--tab column] [--daemon] [-n sec] 'command' ['commands'...] " << std::endl;
    _Exit(0);
}


int
main(int argc, char *argv[])
try
{
    if (argc < 2)
        usage();

    char **opt = &argv[1];

    auto is_opt = [](const char *arg, const char *opt, const char *opt2 = nullptr, const char *opt3 = nullptr)
    {
        return std::strcmp(arg, opt) == 0 ||
                (opt2 ? std::strcmp(arg, opt2) == 0 : false) ||
                (opt3 ? std::strcmp(arg, opt3) == 0 : false);
    };


    dwatch::heuristic = heuristic_parser();

    // parse command line option...
    //

    for ( ; opt != (argv + argc) ; opt++)
    {
        if (is_opt(*opt, "-h", "-?", "--help"))
        {
            usage(); return 0;
        }
        if (is_opt(*opt, "-n"))
        {
            option::seconds = atoi(*++opt);
            continue;
        }

        if (is_opt(*opt, "-C"))
        {
            option::cpu = atoi(*++opt);
            continue;
        }

        if (is_opt(*opt, "-c", "--color"))
        {
            option::colors = true;
            continue;
        }
        if (is_opt(*opt, "-d", "--diff"))
        {
            option::diffmode = 1;
            option::showpol++;
            continue;
        }
        if (is_opt(*opt, "-dd"))
        {
            option::diffmode = 1;
            option::showpol += 2;
            continue;
        }
        if (is_opt(*opt, "-ddd"))
        {
            option::diffmode = 1;
            option::showpol += 3;
            continue;
        }
        if (is_opt(*opt, "-x", "--no-banner"))
        {
            option::banner = false;
            continue;
        }
        if (is_opt(*opt, "-z", "--drop-zero"))
        {
            option::drop_zero = true;
            continue;
        }
        if (is_opt(*opt, "-i", "--interval"))
        {
            option::nominal_interval = std::chrono::milliseconds(atoi(*++opt));
            continue;
        }
        if (is_opt(*opt, "-t", "--trace"))
        {
            option::datafile.assign(*++opt);
            continue;
        }
        if (is_opt(*opt, "--tab"))
        {
            option::tab = strtoul(*++opt, nullptr, 0);
            continue;
        }
        if (is_opt(*opt, "--daemon"))
        {
            option::daemon = true;
            continue;
        }
        if (is_opt(*opt, "-e", "--heuristic"))
        {
            dwatch::heuristic.target<heuristic_parser>()->next();
            continue;
        }
        if (is_opt(*opt, "-ee"))
        {
            dwatch::heuristic.target<heuristic_parser>()->next(2);
            continue;
        }
        if (is_opt(*opt, "-eee"))
        {
            dwatch::heuristic.target<heuristic_parser>()->next(3);
            continue;
        }

        break;
    }

    if (opt == (argv + argc))
        throw std::runtime_error("missing argument");


    if ((signal(SIGQUIT, signal_handler) == SIG_ERR) ||
        (signal(SIGTSTP, signal_handler) == SIG_ERR) ||
        (signal(SIGWINCH, signal_handler) == SIG_ERR)
       )
        throw std::runtime_error("signal");

    if (option::daemon && option::datafile.empty())
        throw std::runtime_error("--daemon option meaningless without --trace");

    if (option::daemon) daemon(1,0);

    std::vector<std::string> commands(opt, argv+argc);

    return main_loop(commands);
}
catch(std::exception &e)
{
    std::cerr << __progname << ": " << e.what() << std::endl;
}

