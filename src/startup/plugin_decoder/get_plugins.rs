
use std::path::Path ;
use std::fs ;

use super::get_manifest::get_manifest ;
use super::plugin::Plugin ;
use super::DecoderError ;




pub fn get_plugins(

    plugins_dir: &Path,

) -> Result<Vec<Plugin>, DecoderError> {

    let plugin_dirs = fs::read_dir( plugins_dir )
        .map_err(| err | DecoderError::CannotReadPluginDirectory( err ))?;

    plugin_dirs.filter_map(| entry | match entry {
        Ok( dir ) => match dir.path().is_dir() {
            true => Some( match get_manifest( &dir.path() ) {
                Ok( plugin ) => Ok( plugin ),
                Err( e ) => Err( e ),
            }),
            false => None,
        }
        Err( e ) => Some( Err( e ).map_err(| err | DecoderError::InaccesiblePlugin( err )) )
    }).collect()

}