cmake_minimum_required(VERSION 3.24)
project(llw_windows_pack_local C)

add_library(llw_pack_local_helper SHARED helper.c)
set_target_properties(llw_pack_local_helper PROPERTIES WINDOWS_EXPORT_ALL_SYMBOLS ON)

add_library(local_llm_runtime SHARED runtime.c)
target_compile_definitions(local_llm_runtime PRIVATE LLW_RUNTIME_BUILD)
target_include_directories(local_llm_runtime PRIVATE "${CMAKE_CURRENT_LIST_DIR}/../../../include")
target_link_libraries(local_llm_runtime PRIVATE llw_pack_local_helper)

if(MSVC)
  target_compile_options(llw_pack_local_helper PRIVATE /W4 /WX)
  target_compile_options(local_llm_runtime PRIVATE /W4 /WX)
else()
  target_compile_options(llw_pack_local_helper PRIVATE -Wall -Wextra -Werror)
  target_compile_options(local_llm_runtime PRIVATE -Wall -Wextra -Werror)
endif()
