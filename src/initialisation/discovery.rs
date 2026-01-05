
const PLUGINS_DIR: &str = "./appdata/plugins";
const INTERFACES_DIR: &str = "./appdata/interfaces";

mod get_plugins ;
mod discover_all ;
mod get_interfaces ;
mod raw_plugin_data ;
mod raw_interface_data ;

pub use discover_all::{ discover_all, DiscoveryError, DiscoveryFailure };
pub use raw_plugin_data::{ RawPluginData, ManifestReadError };
pub use raw_interface_data::{ RawInterfaceData, InterfaceCardinality, InterfaceParseError, FunctionData, FunctionReturnType };
use get_plugins::{ try_get_all_cached_plugins, try_download_plugins, try_get_used_interfaces };
use get_interfaces::{ try_into_socket_map, try_get_all_interfaces_from_cache, try_download_all_interfaces };