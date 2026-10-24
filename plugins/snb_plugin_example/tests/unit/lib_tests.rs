use super::*;
use snb_core::plugin::Version;
use std::ffi::CStr;

#[test]
fn test_plugin_ffi() {
    let ptr = create_plugin();
    let cell = unsafe { snb_core::plugin::PluginCell::new(ptr, destroy_plugin, Box::new(())) };

    assert_eq!(cell.name(), "MyPlugin");
    assert_eq!(
        cell.version(),
        Version {
            major: 0,
            minor: 1,
            patch: 0,
        }
    );

    let abi = unsafe { CStr::from_ptr(plugin_abi()).to_str().unwrap() };
    assert_eq!(abi, snb_core::plugin::snb_plugin_abi().to_string());
}
