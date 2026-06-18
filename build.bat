@echo off
REM Gnirehtet build script for Windows.
REM Commands: build, run, test, apk, clean

if "%1"=="" goto usage
if "%1"=="build" goto build
if "%1"=="run" goto run
if "%1"=="test" goto test
if "%1"=="apk" goto apk
if "%1"=="clean" goto clean
goto usage

:build
echo Building gnirehtet binary...
cargo build --release --manifest-path relay-rust\Cargo.toml
goto end

:run
echo Starting gnirehtet...
cargo run --release --manifest-path relay-rust\Cargo.toml -- run
goto end

:test
echo Running tests...
cargo test --manifest-path relay-rust\Cargo.toml
goto end

:apk
echo Building APK...
call gradlew.bat :app:assembleRelease
goto end

:clean
echo Cleaning...
cargo clean --manifest-path relay-rust\Cargo.toml
call gradlew.bat clean 2>nul
goto end

:usage
echo Usage: build.bat [build^|run^|test^|apk^|clean]
echo   build  - Build the release binary
echo   run    - Build and run gnirehtet
echo   test   - Run all tests
echo   apk    - Build the Android APK
echo   clean  - Clean build artifacts
goto end

:end
