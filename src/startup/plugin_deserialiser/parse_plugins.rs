use crate::startup::plugin_discovery::RawPluginData ;
use crate::utils::ResultList ;
use super::plugin::Plugin ;
use super::DecoderError ;
use super::parse_plugin::parse_plugin ;



pub fn parse_plugins(

    plugin_data: Vec<RawPluginData>,

) -> ResultList<Plugin, DecoderError> {

    plugin_data.into_iter().map( parse_plugin ).collect()

}