cmake_minimum_required(VERSION 3.22.1)

project(s5terminal LANGUAGES CXX)

add_library(ghostty-vt SHARED IMPORTED)
set_target_properties(
    ghostty-vt
    PROPERTIES IMPORTED_LOCATION
               "${CMAKE_CURRENT_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}/libghostty-vt.so"
)

add_library(s5terminal SHARED s5_terminal_jni.cpp)

target_include_directories(
    s5terminal
    PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../../../../../../native/libghostty-vt/include"
)

target_compile_features(s5terminal PRIVATE cxx_std_20)
target_link_options(
    s5terminal PRIVATE "-Wl,-z,common-page-size=16384" "-Wl,-z,max-page-size=16384"
)
target_link_libraries(s5terminal PRIVATE ghostty-vt log)
