
use crate::utils::ResultList ;
use super::RawPluginData ;
// use super::download_plugins::download_plugins ;
use super::PluginCacheError ;



const PLUGINS_DIR: &str = "./appdata/plugins" ;

pub fn get_plugins() -> Result<ResultList<RawPluginData, PluginCacheError>, PluginCacheError> {

    // download_plugins( PLUGINS_DIR.into() );

    Ok( std::fs::read_dir( PLUGINS_DIR )?
        .collect::<Result<Vec<std::fs::DirEntry>,_>>()?
        .iter().map( RawPluginData::new )
        .collect::<ResultList<_,_>>()
    )

}
