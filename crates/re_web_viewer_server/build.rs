#![allow(clippy::unwrap_used)]

use re_build_tools::{
    get_and_track_env_var, is_tracked_env_var_set, rebuild_if_crate_changed, rerun_if_changed,
};

fn main() {
    if !is_tracked_env_var_set("IS_IN_RERUN_WORKSPACE") {
        // Only run if we are in the rerun workspace, not on users machines.
        return;
    }
    if is_tracked_env_var_set("RERUN_IS_PUBLISHING") {
        // We don't need to rebuild - we should have done so beforehand!
        // See `RELEASES.md`
        return;
    }

    // Rebuild the web-viewer Wasm,
    // because the web_server library bundles it with `include_bytes!`.

    rerun_if_changed("../../web_viewer/favicon.svg");
    rerun_if_changed("../../web_viewer/index.html");
    rerun_if_changed("../../web_viewer/sw.js");

    // We implicitly depend on re_viewer, which means we also implicitly depend on
    // all of its direct and indirect dependencies (which are potentially in-workspace
    // or patched!).
    rebuild_if_crate_changed("re_viewer");

    if get_and_track_env_var("CARGO_FEATURE___CI").is_ok() {
        // If the `__ci` feature is set we skip building the web viewer wasm, saving a lot of time.
        // This feature is set on CI (hence the name), but also with `--all-features`, which is set by rust analyzer, bacon, etc.
        eprintln!("__ci feature detected: Skipping building of web viewer wasm.");
    } else {
        let release = std::env::var("PROFILE").unwrap() == "release";
        if let Err(err) =
            re_build_web_viewer::build(release, is_tracked_env_var_set("RERUN_BUILD_WEBGPU"))
        {
            panic!("Failed to build web viewer: {}", re_error::format(err));
        }
    }
}
