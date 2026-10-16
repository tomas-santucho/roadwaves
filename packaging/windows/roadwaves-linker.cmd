@echo off
set "ROOT=%~dp0..\.."
"%ROOT%\.tools-llvm\llvm-mingw-20260908-ucrt-x86_64\bin\x86_64-w64-mingw32-clang.exe" -L"C:\git-sdk-64\ucrt64\lib\gcc\x86_64-w64-mingw32\16.2.0" -L"C:\git-sdk-64\ucrt64\lib" %*
