cmake_minimum_required(VERSION 3.24)
project(llw_windows_pack_local_fixture LANGUAGES C)

add_library(llw_pack_local_helper SHARED helper.c)
add_library(local_llm_runtime SHARED runtime.c)
target_link_libraries(local_llm_runtime PRIVATE llw_pack_local_helper)
