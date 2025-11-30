
use itertools::Itertools ;

use super::RawPluginData ;
// use super::download_plugins::download_plugins ;
use super::PluginCacheError ;



const PLUGINS_DIR: &str = "./appdata/plugins" ;

pub fn get_plugins() -> Result<( Vec<RawPluginData>, Vec<PluginCacheError> ), PluginCacheError> {

    // download_plugins( PLUGINS_DIR.into() );

    Ok( std::fs::read_dir( PLUGINS_DIR )?
        .collect::<Result<Vec<std::fs::DirEntry>,_>>()?
        .iter().map( RawPluginData::new )
        .partition_result()
    )

}
