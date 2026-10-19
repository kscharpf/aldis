# s390x Linux with Ubuntu's GCC 14 cross compiler (D3): the big-endian host target (VER-030 for
# C++). Tests run under QEMU user mode, as `cross` runs the Rust tests.
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR s390x)
set(CMAKE_C_COMPILER s390x-linux-gnu-gcc-14)
set(CMAKE_CXX_COMPILER s390x-linux-gnu-g++-14)
set(CMAKE_FIND_ROOT_PATH /usr/s390x-linux-gnu)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_CROSSCOMPILING_EMULATOR qemu-s390x -L /usr/s390x-linux-gnu)
