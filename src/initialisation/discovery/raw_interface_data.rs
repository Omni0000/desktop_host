use std::path::{ PathBuf, Path };
use std::collections::HashMap ;
use thiserror::Error ;
use wit_parser::{Function, Resolve, Type};

use crate::initialisation::InterfaceId ;
use super::{ DiscoveryError, INTERFACES_DIR };



#[derive( Debug )]
pub struct RawInterfaceData {
    id: InterfaceId,
    wit_data: InterfaceWitData,
    cardinality: InterfaceCardinality,
}

#[derive( Debug )]
struct InterfaceWitData {
    package: String,
    functions: HashMap<String, Function>,
}

#[derive( Debug, PartialEq, Eq, Copy, Clone )]
pub enum InterfaceCardinality {
    AtMostOne,
    ExactlyOne,
    AtLeastOne,
    Any,
}
impl std::fmt::Display for InterfaceCardinality {
    fn fmt( &self, f: &mut std::fmt::Formatter ) -> std::fmt::Result {
        write!( f, "{:?}", self )
    }
}

// NOTE: wit_parser::decoding::Error is private
type WitParserError = anyhow::Error ;

#[derive( Error, Debug )]
pub enum InterfaceParseError {
    #[error( "IO error: {0}" )] Io( #[from] std::io::Error ),
    #[error( "Regex error: {0}" )] Regex( #[from] regex::Error ),
    #[error( "Wit-parser error: {0}" )] WitParser( WitParserError ),
    #[error( "No root interface" )] NoRootInterface,
    #[error( "No package for root interface" )] NoPackage,
}

impl RawInterfaceData {

    pub fn new( id: InterfaceId ) -> Result<Self, DiscoveryError> {
        let root_path = Self::_root_path( id );
        if Self::is_complete( &root_path ) {
            Ok( Self {
                id,
                wit_data: Self::parse_wit( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
                cardinality: Self::parse_cardinality( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
            })
        } else { Err( DiscoveryError::InterfaceNotInCache( id ))}
    }

    #[inline] fn root_path( &self ) -> PathBuf { Self::_root_path( self.id )}
    #[inline] fn _root_path( id: InterfaceId ) -> PathBuf { PathBuf::from( INTERFACES_DIR ).join( format!( "{}", id ) )}
    #[inline] fn is_complete( root_path: &PathBuf ) -> bool {
        Self::_wit_path( root_path ).is_file()
    }

    #[inline] pub fn id( &self ) -> &InterfaceId { &self.id }

    #[inline] fn wit_path( &self ) -> PathBuf { Self::_wit_path( &self.root_path() )}
    #[inline] fn _wit_path( root_path: &PathBuf ) -> PathBuf { root_path.join( "root.wit" )}

    #[inline] pub fn get_cardinality( &self ) -> &InterfaceCardinality { &self.cardinality }
    #[inline] fn parse_cardinality( _root_path: &PathBuf ) -> Result<InterfaceCardinality, InterfaceParseError> {
        Ok( InterfaceCardinality::ExactlyOne )
    }

    #[inline] pub fn get_package( &self ) -> &String { &self.wit_data.package }
    #[inline] pub fn get_function_names( &self ) -> Vec<&String> { self.wit_data.functions.keys().collect()}
    #[inline] pub fn get_function_return_type( &self, function: &str ) -> Option<Option<Type>> {
        let Some( function ) = self.wit_data.functions.get( function ) else { return None };
        Some( function.result )
    }
    #[inline] fn parse_wit( root_path: &PathBuf ) -> Result<InterfaceWitData, InterfaceParseError> {

        let mut resolve = Resolve::new();
        let _ = resolve.push_path( AsRef::<Path>::as_ref( &root_path ))
            .map_err(| err | InterfaceParseError::WitParser( err ))?;

        let interface = resolve.interfaces.iter().find(|( _, interface )| match &interface.name {
            Some( name ) => name.as_str() == "root",
            Option::None => false,
        }).ok_or( InterfaceParseError::NoRootInterface )?.1;

        let package = resolve.packages
            .get( interface.package.ok_or( InterfaceParseError::NoPackage )? )
            .ok_or( InterfaceParseError::NoPackage )?
            .name.to_string();

        let functions = interface.functions.iter()
            .map(|( _, function )| ( function.name.clone(), function.clone() ))
            .collect();

        Ok( InterfaceWitData { package, functions })
    }
}
