# CMakefile for the OFP4 back-end.
# To be included in the main P4C compiler CMakefile

message(STATUS "Start configuring OFP4 back end")

# Source files
set (P4C_OF_SOURCES
  backend.cpp
  midend.cpp
  p4c-of.cpp
)

# header files
set (P4C_OF_HEADERS
  backend.h
  midend.h
  options.h
)

set (OF_DIST_HEADERS p4include/of_model.p4)

# Files to check using cpplint
add_cpplint_files(${CMAKE_CURRENT_SOURCE_DIR} "${P4C_OF_SOURCES};${P4C_OF_HEADERS}")

build_unified(P4C_OF_SOURCES)

add_executable(p4c-of ${P4C_OF_SOURCES})
target_link_libraries(p4c-of ${P4C_LIBRARIES} ${P4C_LIB_DEPS})

install (TARGETS p4c-of
  RUNTIME DESTINATION ${P4C_RUNTIME_OUTPUT_DIRECTORY})
install (DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/p4include
  DESTINATION ${P4C_ARTIFACTS_OUTPUT_DIRECTORY})

add_custom_target(linkp4cof
  COMMAND ${CMAKE_COMMAND} -E create_symlink ${CMAKE_CURRENT_BINARY_DIR}/p4c-of ${P4C_BINARY_DIR}/p4c-of
  COMMAND ${CMAKE_COMMAND} -E make_directory ${P4C_BINARY_DIR}/p4include &&
          ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/${OF_DIST_HEADERS} ${P4C_BINARY_DIR}/p4include
  COMMAND ${CMAKE_COMMAND} -E create_symlink ${P4C_BINARY_DIR}/p4include ${CMAKE_CURRENT_BINARY_DIR}/p4include
  )

set (OF_XFAIL_TESTS
)

add_dependencies(p4c_driver linkp4cof)

# Program used to run tests
set(OF_DRIVER ${P4C_SOURCE_DIR}/extensions/ofp4/run-of-test.py)

# Tests to run
set (OF_TEST_SUITES "${CMAKE_CURRENT_SOURCE_DIR}/tests/*.p4")
p4c_add_tests("of" ${OF_DRIVER} "${OF_TEST_SUITES}" "${OF_XFAIL_TESTS}")

message(STATUS "Done with configuring OFP4 back end")
