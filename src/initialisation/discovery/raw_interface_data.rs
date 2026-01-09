use std::path::{ PathBuf, Path };
use std::collections::HashMap ;
use std::io::Cursor ;
use thiserror::Error ;
use wit_parser::{ Function, FunctionKind, Resolve, Type, TypeDefKind, TypeId };
use capnp::{ serialize, message::ReaderOptions };

use crate::initialisation::InterfaceId ;
use crate::capnp::common::interface_capnp ;
use crate::capnp::manifest::interface_manifest_capnp::interface_manifest ;
use super::DiscoveryError ;



#[derive( Debug )]
pub struct RawInterfaceData {
    id: InterfaceId,
    manifest_data: Vec<u8>,
    wit_data: InterfaceWitData,
}

#[derive( Debug )]
struct InterfaceWitData {
    package: String,
    functions: HashMap<String, FunctionData>,
    resources: Vec<String>,
}

#[derive( Debug, Clone )]
pub struct FunctionData {
    function: Function,
    return_type: FunctionReturnType,
}
impl FunctionData {
    #[inline] pub fn name( &self ) -> &str { &self.function.name }
    #[inline] pub fn has_return( &self ) -> bool { self.return_type != FunctionReturnType::None }
    #[inline] pub fn return_type( &self ) -> &FunctionReturnType { &self.return_type }
    #[inline] pub fn is_method( &self ) -> bool { match self.function.kind {
        FunctionKind::Freestanding | FunctionKind::Static( _ ) | FunctionKind::Constructor( _ ) => false,
        FunctionKind::Method( _ ) => true,
        FunctionKind::AsyncFreestanding | FunctionKind::AsyncMethod( _ ) | FunctionKind::AsyncStatic( _ )
        => unimplemented!( "Async functions are not yet implemented" ),
    }}
}

#[derive( Debug, Clone, PartialEq )]
pub enum FunctionReturnType {
    None,
    DataNoResource,
    DataWithResources,
}

#[derive( Debug, PartialEq, Eq, Copy, Clone )]
pub enum InterfaceCardinality {
    AtMostOne,
    ExactlyOne,
    AtLeastOne,
    Any,
}
impl std::fmt::Display for InterfaceCardinality {
    fn fmt( &self, f: &mut std::fmt::Formatter ) -> std::fmt::Result { write!( f, "{:?}", self )}
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
    #[error( "Undeclared type: {0:?}" )] UndeclaredType( TypeId ),
    #[cfg( feature = "test" )] #[error( "Toml Parse Error: {0}" )] TomlManifestParseError( toml::de::Error ),
    #[cfg( feature = "test" )] #[error( "Capnp Error: {0}" )] Capnp( capnp::Error ),
}

#[derive( Error, Debug )]
pub enum InterfaceManifestReadError {
    #[error( "IO error: {0}" )] Io( #[from] std::io::Error ),
    #[error( "Capnp error: {0}" )] Capnp( #[from] capnp::Error ),
    #[error( "Invalid manifest" )] InvalidManifest,
}



impl RawInterfaceData {

    const WIT_FILE: &str = "root.wit" ;
    const MANIFEST_FILE: &str = "manifest.bin" ;
    /* TEST */ const MANIFEST_TOML_FILE: &str = "manifest.toml" ;

    const ROOT_INTERFACE: &str = "root" ;

    pub fn new( source: &PathBuf, id: InterfaceId ) -> Result<Self, DiscoveryError> {
        let root_path = Self::root_path( source, id );
        if Self::is_complete( &root_path ) { Ok( Self {
            id,
            manifest_data: Self::get_manifest_data( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
            wit_data: Self::parse_wit( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
        }) } else { Err( DiscoveryError::InterfaceNotInCache( id ))}
    }

    #[inline] fn root_path( source: &PathBuf, id: InterfaceId ) -> PathBuf { source.join( id.to_string() )}
    #[inline] fn is_complete( root_path: &PathBuf ) -> bool {
        Self::wit_path( root_path ).is_file()
        && Self::manifest_path( root_path ).is_file()
    }

    #[inline] fn wit_path( root_path: &PathBuf ) -> PathBuf { root_path.join( Self::WIT_FILE )}
    #[inline] fn manifest_path( root_path: &PathBuf ) -> PathBuf {
        #[cfg( not( feature = "test" ))] {
            root_path.join( Self::MANIFEST_FILE )
        }
        #[cfg( feature = "test" )] {
            let bin_path = root_path.join( Self::MANIFEST_FILE );
            if bin_path.is_file() { bin_path }
            else { root_path.join( Self::MANIFEST_TOML_FILE )}
        }
    }

    #[inline] pub fn id( &self ) -> &InterfaceId { &self.id }

    #[inline] pub fn get_package( &self ) -> &String { &self.wit_data.package }
    #[inline] pub fn get_functions( &self ) -> Vec<&FunctionData> { self.wit_data.functions.values().collect() }
    #[inline] pub fn get_resources( &self ) -> &Vec<String> { &self.wit_data.resources }

    #[inline] fn get_manifest_data( root_path: &PathBuf ) -> Result<Vec<u8>, InterfaceParseError> {
        #[cfg( not( feature = "test" ))] {
            std::fs::read( Self::manifest_path( &root_path )).map_err( InterfaceParseError::Io )
        }
        #[cfg( feature = "test" )] {
            let bin_path = root_path.join( Self::MANIFEST_FILE );
            if bin_path.is_file() {
                std::fs::read( Self::manifest_path( &root_path )).map_err( InterfaceParseError::Io )
            } else {
                let text = std::fs::read_to_string( Self::manifest_path( &root_path )).map_err( InterfaceParseError::Io )?;
                let data: test_conversion::DeserialisableInterfaceManifest = toml::from_str( &text )
                    .map_err( InterfaceParseError::TomlManifestParseError )?;
                let data: test_conversion::SerialisableInterfaceManifest = data.into();
                let mut message = capnp::message::Builder::new_default();
                capnp_conv::Writable::write( &data, message.init_root());
                let mut buffer = Vec::new();
                capnp::serialize::write_message( &mut buffer, &message ).map_err( InterfaceParseError::Capnp )?;
                Ok( buffer )
            }
        }
    }

    #[inline] fn parse_wit( root_path: &PathBuf ) -> Result<InterfaceWitData, InterfaceParseError> {

        let mut resolve = Resolve::new();
        let _ = resolve.push_path( AsRef::<Path>::as_ref( &root_path ))
            .map_err(| err | InterfaceParseError::WitParser( err ))?;

        let interface = resolve.interfaces.iter().find(|( _, interface )| match &interface.name {
            Some( name ) => name.as_str() == Self::ROOT_INTERFACE ,
            Option::None => false,
        }).ok_or( InterfaceParseError::NoRootInterface )?.1;

        let package = resolve.packages
            .get( interface.package.ok_or( InterfaceParseError::NoPackage )? )
            .ok_or( InterfaceParseError::NoPackage )?
            .name.to_string();

        let functions = interface.functions.iter()
            .map(|( _, function )| Ok(( function.name.clone(), FunctionData {
                function: function.clone(),
                return_type: parse_return_type( &resolve, function.result )?,
            })))
            .collect::<Result<_,InterfaceParseError>>()?;

        let resources = interface.types.iter().filter_map(|( name, wit_type_id )| match resolve.types.get( *wit_type_id ) {
            Option::None => Some( Err( InterfaceParseError::UndeclaredType( wit_type_id.clone() ) )),
            Some( wit_type ) if wit_type.kind == wit_parser::TypeDefKind::Resource => Some( Ok( name.to_string() )),
            _ => None,
        }).collect::<Result<_, InterfaceParseError>>()?;

        Ok( InterfaceWitData { package, functions, resources })

    }

    #[inline( always )] pub fn get_cardinality( &self ) -> Result<InterfaceCardinality, InterfaceManifestReadError> {

        let reader = serialize::read_message( Cursor::new( &self.manifest_data ), ReaderOptions::new() )
            .map_err( InterfaceManifestReadError::Capnp )?;
        let root = reader.get_root::<interface_manifest::Reader>()
            .map_err( InterfaceManifestReadError::Capnp )?;

        let cardinality = root
            .get_cardinality()
            .map_err(|_| InterfaceManifestReadError::InvalidManifest )?;

        Ok( match cardinality {
            interface_capnp::InterfaceCardinality::One => InterfaceCardinality::ExactlyOne,
            interface_capnp::InterfaceCardinality::Many => InterfaceCardinality::Any,
            interface_capnp::InterfaceCardinality::AtMostOne => InterfaceCardinality::AtMostOne,
            interface_capnp::InterfaceCardinality::AtLeastOne => InterfaceCardinality::AtLeastOne,
        })

    }

}

#[inline] fn parse_return_type( resolve: &Resolve, result: Option<Type> ) -> Result<FunctionReturnType, InterfaceParseError> {
    
    let Some( return_type ) = result else { return Ok( FunctionReturnType::None )};
    Ok( match has_resource( resolve, return_type )? {
        false => FunctionReturnType::DataNoResource,
        true => FunctionReturnType::DataWithResources,
    })

}

#[inline] fn has_resource( resolve: &Resolve, wit_type: wit_parser::Type ) -> Result<bool, InterfaceParseError> {

    Ok( match wit_type {
        wit_parser::Type::Id( id ) => match &resolve.types.get( id )
            .ok_or_else(|| InterfaceParseError::UndeclaredType( id ))?
            .kind
        {
            TypeDefKind::Resource
            | TypeDefKind::Handle( wit_parser::Handle::Own( _ )) => true,
            
            TypeDefKind::Handle( wit_parser::Handle::Borrow( _ ))
            | TypeDefKind::Flags( _ )
            | TypeDefKind::Enum( _ )
            | TypeDefKind::Future( Option::None )
            | TypeDefKind::Stream( Option::None )
            | TypeDefKind::Unknown => false,
            
            TypeDefKind::Option( wit_type )
            | TypeDefKind::List( wit_type )
            | TypeDefKind::FixedSizeList( wit_type, _ )
            | TypeDefKind::Future( Some( wit_type ))
            | TypeDefKind::Stream( Some( wit_type ))
            | TypeDefKind::Type( wit_type ) => has_resource( resolve, *wit_type )?,
            
            TypeDefKind::Map( key_type, value_type ) =>
                has_resource( resolve, *key_type )?
                || has_resource( resolve, *value_type )?,
            
            TypeDefKind::Result( result ) =>
                ( match result.ok { Some( wit_type ) => has_resource( resolve, wit_type )?, _ => false, })
                || match result.ok { Some( wit_type ) => has_resource( resolve, wit_type )?, _ => false, },

            TypeDefKind::Record( record ) => record.fields.iter().try_fold( false, | acc, field |
                Result::<_, InterfaceParseError>::Ok( acc || has_resource( resolve, field.ty )? )
            )?,
            
            TypeDefKind::Tuple( tuple ) => tuple.types.iter().try_fold( false, | acc, &item |
                Result::<_, InterfaceParseError>::Ok( acc || has_resource( resolve, item )? )
            )?,
            
            TypeDefKind::Variant( variant ) => variant.cases.iter().try_fold( false, | acc, case |
                Result::<_, InterfaceParseError>::Ok( acc || match case.ty {
                    Some( wit_type ) => has_resource( resolve, wit_type )?,
                    Option::None => false,
                })
            )?,
        },
        _ => false,
    })

}

#[cfg( feature = "test" )]
mod test_conversion {

    use capnp_conv::capnp_conv ;
    use serde::Deserialize ;

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::interface_capnp::interface_id )]
    pub struct DeserialisableInterfaceId {
        pub id: u64,
    }

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::version_capnp::version )]
    pub struct DeserialisableVersion {
        pub major: u16,
        pub minor: u16,
        pub patch: u16,
    }

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::interface_capnp::InterfaceCardinality )]
    pub enum DeserialisableInterfaceCardinality {
        One,
        Many,
        AtMostOne,
        AtLeastOne,
    }

    #[derive( Deserialize )]
    pub struct DeserialisableInterfaceManifest {
        pub id: DeserialisableInterfaceId,
        pub version: DeserialisableVersion,
        pub cardinality: DeserialisableInterfaceCardinality,
    }
    #[capnp_conv( crate::capnp::manifest::interface_manifest_capnp::interface_manifest )]
    pub struct SerialisableInterfaceManifest {
        pub id: DeserialisableInterfaceId,
        pub version: DeserialisableVersion,
        #[capnp_conv( type = "enum" )] pub cardinality: crate::capnp::common::interface_capnp::InterfaceCardinality,
    }
    impl Into<SerialisableInterfaceManifest> for DeserialisableInterfaceManifest {
        fn into( self ) -> SerialisableInterfaceManifest {
            SerialisableInterfaceManifest {
                id: self.id,
                version: self.version,
                cardinality: self.cardinality.into(),
            }
        }
    }

}
