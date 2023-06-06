use std::{io::Write, path::PathBuf};

// ## lib_name: "protobufd"
// ## dir: "..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/protobuf_fetch-build"
// rustc-link-search=native=..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/protobuf_fetch-build
// rustc-link-lib=protobufd
// ## lib_name: "libyosys"
// ## dir: "..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/yosys_fetch-build"
// rustc-link-search=native=..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/yosys_fetch-build
// rustc-link-lib=libyosys
// ## lib_name: "xxhash"
// ## dir: "..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/xxhash-build"
// rustc-link-search=native=..../api_circuits/target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/xxhash-build
// rustc-link-lib=xxhash
fn parse_lib_path_dir_and_name(static_lib_str: &str) -> (PathBuf, String, bool, bool, bool) {
    let static_lib_path = std::path::Path::new(static_lib_str);

    // NOTE: file_stem only split eg "libprotobufd.so.3.19.4.0" -> "libprotobufd.so.3.19.4"
    // but that is NOT what we want (ie "libprotobufd")
    // TODO use "file_prefix" https://github.com/rust-lang/rust/issues/86319
    let liblib_name = static_lib_path.my_file_prefix().unwrap();
    let liblib_name_str: String = liblib_name.to_str().unwrap().into();
    let lib_name_str = liblib_name_str
        .strip_prefix("lib")
        .unwrap_or(&liblib_name_str);

    // basically:
    // - input = /.../target/debug/build/lib-circuits-wrapper-49025516ce40925e/out/build/_deps/glog-build/libglogd.so.0.6.0
    // - get the extension: a (or "so.3.19.4" or "so" etc)
    // NOTE: extension DOES NOT work(same issue than file_stem)
    // eg ".../libglogd.so.0.6.0".extension() == "0" (ie the part after the last dot)
    // and we NEED "so" (ie the part after the FIRST dot)
    let file_with_ext = static_lib_path.file_name().unwrap();
    let full_ext = file_with_ext
        .to_str()
        .unwrap()
        .trim_start_matches(&liblib_name_str);
    let is_static = full_ext.starts_with(".a");

    let dir = static_lib_path.parent().unwrap();

    // COULD probably have a more foolproof system by using the IMPORTED property in CMake
    // and writing that to a different file(or the same one with a prefix/suffix?)
    // NOTE: be sure that the prefix does not conflict with the Dockerfile WORKDIR /usr/src/app
    let is_system = dir.starts_with("/usr/lib/");

    let is_framework = static_lib_str.ends_with(".framework");

    (
        dir.to_path_buf(),
        lib_name_str.to_string(),
        is_static,
        is_system,
        is_framework,
    )
}

// Parse the content of "cmake_generated_rust_wrapper_libs" which SHOULD have
// been generated by our CMake function.
// It is expected to contain a list of space separated libraries eg:
// "/full/path/build/liblib1.so /full/path/build/liblib2.a /usr/lib/x86_64-linux-gnu/libpng16.so.16.37.0"
// etc
fn read_cmake_generated_to_output(
    cmake_generated_rust_wrapper_libs_str: &str,
    output: &mut impl Write,
) {
    // Previous version was globing all .a and .so in the build dir but it only worked for SHARED dependencies.
    // That is b/c when linking STATIC libs order matters! So we must get a proper list from CMake.
    for static_lib_str in cmake_generated_rust_wrapper_libs_str
        .split(&[' ', '\n'][..])
        .filter(|&x| !x.is_empty())
    {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name(static_lib_str);
        // WARNING: we MUST add to the linker path:
        // - NON system libs (obviously) wether SHARED or STATIC
        // - system STATIC libs eg /usr/lib/x86_64-linux-gnu/libboost_filesystem.a else
        //  "error: could not find native static library `boost_filesystem`, perhaps an -L flag is missing?"
        if (!is_system && !is_framework) || is_static {
            writeln!(output, "cargo:rustc-link-search=native={}", dir.display()).unwrap();
        }

        writeln!(
            output,
            "cargo:rustc-link-lib={}={}",
            if is_framework {
                "framework"
            } else if is_static {
                "static"
            } else {
                "dylib"
            },
            lib_name_str
        )
        .unwrap();
    }
}

pub fn read_cmake_generated(cmake_generated_rust_wrapper_libs_str: &str) {
    read_cmake_generated_to_output(
        cmake_generated_rust_wrapper_libs_str,
        &mut std::io::stdout(),
    )
}

////////////////////////////////////////////////////////////////////////////////
/// TEMP
/// Implement "file_prefix"
/// copy pasted from https://github.com/rust-lang/rust/issues/86319
use std::ffi::OsStr;

trait HasMyFilePrefix {
    fn my_file_prefix(&self) -> Option<&OsStr>;
}

impl HasMyFilePrefix for std::path::Path {
    fn my_file_prefix(&self) -> Option<&OsStr> {
        self.file_name()
            .map(split_file_at_dot)
            .map(|(before, _after)| before)
    }
}

fn split_file_at_dot(file: &OsStr) -> (&OsStr, Option<&OsStr>) {
    let slice = os_str_as_u8_slice(file);
    if slice == b".." {
        return (file, None);
    }

    // The unsafety here stems from converting between &OsStr and &[u8]
    // and back. This is safe to do because (1) we only look at ASCII
    // contents of the encoding and (2) new &OsStr values are produced
    // only from ASCII-bounded slices of existing &OsStr values.
    let i = match slice[1..].iter().position(|b| *b == b'.') {
        Some(i) => i + 1,
        None => return (file, None),
    };
    let before = &slice[..i];
    let after = &slice[i + 1..];
    unsafe { (u8_slice_as_os_str(before), Some(u8_slice_as_os_str(after))) }
}

fn os_str_as_u8_slice(s: &OsStr) -> &[u8] {
    unsafe { &*(s as *const OsStr as *const [u8]) }
}
unsafe fn u8_slice_as_os_str(s: &[u8]) -> &OsStr {
    // SAFETY: see the comment of `os_str_as_u8_slice`
    {
        &*(s as *const [u8] as *const OsStr)
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::{parse_lib_path_dir_and_name, read_cmake_generated_to_output};

    #[test]
    fn parse_local_lib_static_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/some/path/liblibstatic.a");
        assert_eq!(dir.as_os_str(), "/some/path");
        assert_eq!(lib_name_str, "libstatic");
        assert!(is_static);
        assert!(!is_system);
        assert!(!is_framework);
    }

    #[test]
    fn parse_local_lib_shared_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/some/path/liblibshared.so");
        assert_eq!(dir.as_os_str(), "/some/path");
        assert_eq!(lib_name_str, "libshared");
        assert!(!is_static);
        assert!(!is_system);
        assert!(!is_framework);
    }

    #[test]
    fn parse_local_lib_shared_with_soversion_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/some/path/liblibshared.so.1.2.3");
        assert_eq!(dir.as_os_str(), "/some/path");
        assert_eq!(lib_name_str, "libshared");
        assert!(!is_static);
        assert!(!is_system);
        assert!(!is_framework);
    }

    #[test]
    fn parse_system_lib_static_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/usr/lib/libsystem1.a");
        assert_eq!(dir.as_os_str(), "/usr/lib");
        assert_eq!(lib_name_str, "system1");
        assert!(is_static);
        assert!(is_system);
        assert!(!is_framework);
    }

    #[test]
    fn parse_system_lib_shared_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/usr/lib/libsystem2.so");
        assert_eq!(dir.as_os_str(), "/usr/lib");
        assert_eq!(lib_name_str, "system2");
        assert!(!is_static);
        assert!(is_system);
        assert!(!is_framework);
    }

    #[test]
    fn parse_framework_ok() {
        let (dir, lib_name_str, is_static, is_system, is_framework) =
            parse_lib_path_dir_and_name("/AAA/BBB.framework");
        assert_eq!(dir.as_os_str(), "/AAA");
        assert_eq!(lib_name_str, "BBB");
        assert!(!is_static);
        assert!(!is_system);
        assert!(is_framework);
    }

    #[test]
    fn test_read_cmake_generated_to_output() {
        let input = "/some/libA.a /some/libB.so";
        let mut stdout = Vec::new();
        read_cmake_generated_to_output(input, &mut stdout);

        assert_eq!(
            std::str::from_utf8(&stdout).unwrap(),
            "cargo:rustc-link-search=native=/some\n\
        cargo:rustc-link-lib=static=A\n\
        cargo:rustc-link-search=native=/some\n\
        cargo:rustc-link-lib=dylib=B\n"
        );
    }

    // no need to touch "rustc-link-search" to link with eg "/usr/lib/x86_64-linux-gnu/libpng16.so.16.37.0"
    // simply "cargo:rustc-link-lib=dylib=png16.so" is OK
    #[test]
    fn test_read_cmake_generated_to_output_system_shared_no_rustc_link_search() {
        let input = "/usr/lib/x86_64-linux-gnu/libpng16.so.16.37.0";
        let mut stdout = Vec::new();
        read_cmake_generated_to_output(input, &mut stdout);

        assert_eq!(
            std::str::from_utf8(&stdout).unwrap(),
            "cargo:rustc-link-lib=dylib=png16\n"
        );
    }

    // BUT system STATIC libs require "rustc-link-search"??
    #[test]
    fn test_read_cmake_generated_to_output_system_static_rustc_link_search() {
        let input = "/usr/lib/x86_64-linux-gnu/libpng16.a";
        let mut stdout = Vec::new();
        read_cmake_generated_to_output(input, &mut stdout);

        assert_eq!(
            std::str::from_utf8(&stdout).unwrap(),
            "cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu\n\
            cargo:rustc-link-lib=static=png16\n"
        );
    }
}