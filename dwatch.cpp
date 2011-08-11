#include <sys/types.h>
#include <sys/wait.h>

#include <iostream>
#include <algorithm>
#include <chrono>
#include <limits>

#include <snippet>

const char * const CLEAR = "\033[2J\033[1;1H";
const char * const BOLD  = "\E[1m";
const char * const RESET = "\E[0m";
const char * const BLUE  = "\E[1;34m";
const char * const RED   = "\E[31m";

extern const char *__progname;

typedef std::pair<size_t, size_t>  range_type;

/* global options */

int  g_seconds = std::numeric_limits<int>::max();
bool g_color = false;

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
            return isspace(c) || c == ',' || c == ':' || c == ';'; 
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


uint32_t
hash_line(const char *s, const std::vector<range_type> &xs)
{
    const char *s_end = s + strlen(s);
    std::string str;
    str.reserve(s_end-s);
    
    size_t index = 0;
    std::for_each(s, s_end, [&](char c) { 
                  if (!in_range(index++, xs)) 
                      str.push_back(c); 
                  }); 
    
    return std::hash<std::string>()(str);
}


void
stream_line(std::ostream &out, const std::vector<std::string> &i, const std::vector<uint64_t> &m, const std::vector<uint64_t> &d, std::vector<range_type> &xs)
{
    bool m_first = (!xs.empty() && xs[0].first == 0);

    auto it = i.begin(), it_e = i.end();
    auto mt = m.begin(), mt_e = m.end();
    auto dt = d.begin(), dt_e = d.end();

    for(; (it != it_e) || (mt != mt_e);)
    {
        if (m_first) 
        {
            if ( mt != mt_e ) out << *mt++ << "[" << (g_color ? BOLD : "") << *dt++ << "/sec" << RESET << "]";
            if ( it != it_e ) out << *it++;
        }
        else 
        {
            if ( it != it_e ) out << *it++;
            if ( mt != mt_e ) out << *mt++ << "[" << (g_color ? BOLD : "") << *dt++ << "/sec" << RESET << "]";
        }
    }
}   


void show_line(size_t n, const char *line)
{
    static std::unordered_map<size_t, std::tuple<uint32_t, std::vector<range_type>, std::vector<uint64_t> >> dmap;

    auto ranges = get_ranges(line);
    auto h      = hash_line(line, ranges);
    auto values = get_mutables(line, ranges);
    auto it     = dmap.find(n);

    if (it == dmap.end() || 
        ranges.empty() ||
        std::get<0>(it->second) != h || 
        std::get<1>(it->second).size() != ranges.size() )
    {
        std::cout << line;
    }
    else 
    {
        std::vector<uint64_t> diff(values.size());

        std::transform(values.begin(), values.end(),
                       std::get<2>(it->second).begin(), diff.begin(), std::minus<uint64_t>());

        stream_line(std::cout, get_immutables(line, ranges), values, diff, ranges);
    }

    dmap[n] = std::make_tuple(h, ranges, values); 
}


int main_loop(const char *command)
{
    for(int n=0; n < g_seconds; ++n)
    {
        std::cout << CLEAR << "Every " << n << "s: " << command << std::endl;

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

            execl("/bin/sh", "sh", "-c", command, NULL);
            _exit(127);
        }
        else { /* parent */

            ::close(fds[1]); /* for writing */

            FILE * fp = ::fdopen(fds[0], "r");
            char *line; size_t len = 0; ssize_t read;

            /* dump output */

            size_t n = 0;
            while( (read = ::getline(&line, &len, fp)) != -1 )
            {   
                show_line(n++,line); 
            }

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

        std::this_thread::sleep_for(std::chrono::seconds(1));
    }

    return 0;
}                   

void usage()
{
    std::cout << "usage: " << __progname << " [-h] [-c|--color] [-n sec] command [args...]" << std::endl;
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
        if (!std::strcmp(*opt, "-c") ||
            !std::strcmp(*opt, "--color"))
        {
            g_color = true;
            continue;
        }

        break;
        std::cout << "option: " << *opt << std::endl;
    }

    return main_loop(*opt);
}



