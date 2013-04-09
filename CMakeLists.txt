cmake_minimum_required(VERSION 2.8)

if (NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Choose the type of build: Release or Debug" FORCE)
endif()

set (CMAKE_CXX_FLAGS_DEBUG   "${CMAKE_CXX_FLAGS} -std=c++11 -D_GLIBCXX_DEBUG -g -O0 -Wall -Wextra")
set (CMAKE_CXX_FLAGS_RELEASE "${CMAKE_CXX_FLAGS} -std=c++11 -DNDEBUG -O3 -march=native -Wall -Wextra")

add_executable(dwatch dwatch.cpp)

target_link_libraries(dwatch -pthread)
