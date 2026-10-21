# Android spike build environment (source it from Git Bash): NDK r29, the SDK's CMake,
# and the libclang workaround rquickjs-sys's bindgen needs on Windows (docs/android-spike.md, check 6).
export ANDROID_HOME=D:/android-sdk
export ANDROID_NDK_HOME=D:/android-sdk/ndk/29.0.14206865
export CARGO_TARGET_DIR=D:/UIImprovement/ml-android-spike/target
export PATH="/d/android-sdk/cmake/3.31.6/bin:$PATH"
NDK_LLVM="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64"
export LIBCLANG_PATH="$NDK_LLVM/bin"
# libclang.dll from the NDK can't find its own resource headers (stdbool.h) on Windows.
export C_INCLUDE_PATH="$NDK_LLVM/lib/clang/21/include"
