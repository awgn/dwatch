#include <sys/types.h>
#include <sys/wait.h>

#include <iostream>
#include <algorithm>
#include <chrono>

#include <snippet>

const char * CLEAR = "\033[2J\033[1;1H";

extern const char *__progname;

typedef std::pair<size_t, size_t>  range_type;

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
complement(const std::vector<range_type> &iv, size_t size)
{
    std::vector<range_type> ic;
    size_t first = 0;

    for(const range_type &ip : iv)
    {
        ic.push_back(std::make_pair(first, ip.first));
        first = ip.second;
    }
    ic.push_back(std::make_pair(first, size));

    ic.erase(std::remove_if(ic.begin(), ic.end(), 
                            [](const range_type &r) { return r.first == r.second; }), ic.end());
    return ic;
}


inline bool 
in_range(std::string::size_type i, const std::vector<range_type> &points)
{
    auto e = points.end();

    std::function<bool(std::vector<range_type>::const_iterator)> 
    predicate = [&](std::vector<range_type>::const_iterator r)
       {
           if (r == e || i < r->first)
               return false;
           else
               return (i >= r->first && i < r->second) ? true : predicate(r+1); 
       };

    return predicate( points.cbegin() );

    // return std::accumulate(points.begin(), points.end(), false, 
    //                        [&](bool acc, const range_type &point) {
    //                        return acc ||  (i >= point.first && i < point.second);
    //                        }); 
}


inline std::vector<uint64_t>
get_mutable(const char *str, const std::vector<range_type> &mp)
{
    std::vector<uint64_t> ret;
    for(const range_type &p : mp)
    {    
        ret.push_back(stoi(std::string(str + p.first, str + p.second)));
    }
    return ret;
}                 


inline std::vector<std::string>
get_immutable(const char *str, const std::vector<range_type> &mp)
{
    std::vector<std::string> ret;
    auto ip = complement(mp, strlen(str));

    for(const range_type &p: ip)
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
    
    std::for_each(s, s_end, [&](char c) { if (in_range(c, xs)) str.push_back(c); }); 
    
    return std::hash<std::string>()(str);
}


void
merge_and_stream(std::ostream &out, const std::vector<std::string> &i, const std::vector<uint64_t> &m, std::vector<range_type> &point)
{
    bool m_first = (!point.empty() && point[0].first == 0);

    auto it = i.begin(), it_e = i.end();
    auto mt = m.begin(), mt_e = m.end();

    for(; (it != it_e) || (mt != mt_e);)
    {
        if (m_first) 
        {
            if ( mt != mt_e ) out << *mt++;
            if ( it != it_e ) out << *it++;
        }
        else 
        {
            if ( it != it_e ) out << *it++;
            if ( mt != mt_e ) out << *mt++;
        }
    }
}   


void show_line(const char *line)
{
    auto ranges = get_ranges(line);

    std::cout << ranges << std::endl;
}


int main_loop(const char *command)
{
    for(int n=0;;++n)
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

            while( (read = ::getline(&line, &len, fp)) != -1 )
            {   
                show_line(line); 
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


int
main(int argc, char *argv[])
{
    const char *str_test  = "9 abc:10 11 12,sdfdsf:13 ";
    const char *str_test2 = "1 abc:10 13 12,sdfdsf:110 ";

    auto p = get_ranges(str_test); 
    auto q = complement(p, strlen(str_test));

    std::cout << p << std::endl;
    std::cout << q << std::endl;
    std::cout << complement(q, strlen(str_test)) << std::endl;


    std::cout << more::streamer::sep(".") << get_mutable(str_test, p) << std::endl;
    std::cout << get_immutable(str_test, p)  << std::endl;

    std::cout << str_test << std::endl;    
    merge_and_stream(std::cout, get_immutable(str_test,p), get_mutable(str_test,p), p);
    std::cout << std::endl;
    
    std::cout << std::hex << hash_line(str_test,  p) << std::endl;
    std::cout << std::hex << hash_line(str_test2, get_ranges(str_test2)) << std::endl;

    return 0;
}


void usage()
{
    std::cout << "usage: " << __progname << " [-h] command [args...]" << std::endl;
}


// int
// main(int argc, char *argv[])
// {
//     if (argc < 2) {
//         usage();
//         return 0;
//     }
// 
//     char **opt = &argv[1];
// 
//     // parse command line option...
//     //
// 
//     for ( ; opt != argv + argc ; opt++)
//     {
//         if (!std::strcmp(*opt, "-h") || !std::strcmp(*opt, "--help"))
//         {
//             usage(); return 0;
//         }
// 
//         break;
//         std::cout << "option: " << *opt << std::endl;
//     }
// 
//     return main_loop(*opt);
// }



