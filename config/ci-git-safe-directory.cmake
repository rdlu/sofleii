# CI only: let git read this repo's metadata inside ZMK's build container.
#
# build.yaml passes this file to the left half's build as
# -DCMAKE_PROJECT_INCLUDE_BEFORE, so CMake runs it before every project() call,
# i.e. before any Zephyr module's CMakeLists.txt is processed.
#
# Why: zmk-feature-device-info (#46) runs `git -C ${ZMK_CONFIG} describe
# --dirty --always --tags` at configure time to fill BuildInfo.zmk_config_version.
# ZMK's reusable build-user-config workflow builds as root inside the
# zmk-build-arm container, but the workspace directory is owned by the runner
# user, so git refuses it ("detected dubious ownership") and the module, which
# discards git's stderr, silently reports an empty config version.
# actions/checkout only adds safe.directory to a *temporary* global config, which
# is gone by the build step.
#
# Fix: mark $GITHUB_WORKSPACE as a safe directory through git's GIT_CONFIG_COUNT
# environment variables ("command" scope, which safe.directory honours). CMake's
# ENV is inherited by every execute_process() later in this configure run, and
# by nothing outside it. Outside GitHub Actions this file is a no-op.

if(NOT DEFINED ENV{GITHUB_WORKSPACE} OR "$ENV{GITHUB_WORKSPACE}" STREQUAL "")
  return()
endif()

get_property(_sofleii_safe_dir_done GLOBAL PROPERTY SOFLEII_CI_GIT_SAFE_DIRECTORY)
if(_sofleii_safe_dir_done)
  return()
endif()
set_property(GLOBAL PROPERTY SOFLEII_CI_GIT_SAFE_DIRECTORY TRUE)

# Append to any GIT_CONFIG_* entries already in the environment.
set(_n 0)
if(DEFINED ENV{GIT_CONFIG_COUNT} AND "$ENV{GIT_CONFIG_COUNT}" MATCHES "^[0-9]+$")
  set(_n "$ENV{GIT_CONFIG_COUNT}")
endif()
set(ENV{GIT_CONFIG_KEY_${_n}} "safe.directory")
set(ENV{GIT_CONFIG_VALUE_${_n}} "$ENV{GITHUB_WORKSPACE}")
math(EXPR _n "${_n} + 1")
set(ENV{GIT_CONFIG_COUNT} "${_n}")

# Print what device-info will embed, so the CI log shows it.
execute_process(
  COMMAND git -C "$ENV{GITHUB_WORKSPACE}" describe --dirty --always --tags
  OUTPUT_VARIABLE _sofleii_config_version
  OUTPUT_STRIP_TRAILING_WHITESPACE
  ERROR_VARIABLE _sofleii_config_version_err
  ERROR_STRIP_TRAILING_WHITESPACE
)
message(STATUS "zmk-config git version (device-info): '${_sofleii_config_version}' ${_sofleii_config_version_err}")
