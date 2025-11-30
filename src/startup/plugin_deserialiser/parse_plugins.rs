use itertools::Itertools ;

use crate::startup::plugin_discovery::RawPluginData ;
use super::plugin::Plugin ;
use super::DecoderError ;
use super::parse_plugin::parse_plugin ;



pub fn parse_plugins(

    plugin_data: Vec<RawPluginData>,

) -> ( Vec<Plugin>, Vec<DecoderError> ) {

    plugin_data.into_iter().map( parse_plugin ).partition_result()

}