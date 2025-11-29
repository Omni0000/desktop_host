
use thiserror::Error ;

mod plugin_parser ;
mod plugin_decoder ;
mod interface_decoder ;

use plugin_parser::find_children ;



#[derive( Error, Debug )]
pub enum StartupError {
    #[error("{0}")] DecoderError( #[from] plugin_decoder::DecoderError ),
    #[error("{0}")] ParserError( #[from] plugin_parser::PluginParserError ),
}

pub fn startup() -> Result<(),StartupError> {

    let plugins = plugin_decoder::get_plugins( std::path::Path::new( "./appdata/plugins" ) )?;
    let _socket_map = find_children( plugins )?;
    
    Ok(())

}