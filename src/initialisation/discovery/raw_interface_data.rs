use std::path::{ PathBuf, Path };
use std::collections::HashMap ;
use thiserror::Error ;
use wit_parser::{ Function, FunctionKind, Resolve, Type, TypeDef, TypeDefKind, TypeId };

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
}

impl RawInterfaceData {

    pub fn new( id: InterfaceId ) -> Result<Self, DiscoveryError> {
        let root_path = Self::root_path( id );
        if Self::is_complete( &root_path ) { Ok( Self {
            id,
            wit_data: Self::parse_wit( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
            cardinality: Self::parse_cardinality( &root_path ).map_err(| err | DiscoveryError::FailedToParseInterface( id, err ))?,
        }) } else { Err( DiscoveryError::InterfaceNotInCache( id ))}
    }

    #[inline] fn root_path( id: InterfaceId ) -> PathBuf { PathBuf::from( INTERFACES_DIR ).join( format!( "{}", id ) )}
    #[inline] fn is_complete( root_path: &PathBuf ) -> bool { Self::wit_path( root_path ).is_file() }
    #[inline] fn wit_path( root_path: &PathBuf ) -> PathBuf { root_path.join( "root.wit" )}

    #[inline] pub fn id( &self ) -> &InterfaceId { &self.id }

    #[inline] pub fn get_cardinality( &self ) -> &InterfaceCardinality { &self.cardinality }

    #[inline] pub fn get_package( &self ) -> &String { &self.wit_data.package }
    #[inline] pub fn get_functions( &self ) -> Vec<&FunctionData> { self.wit_data.functions.values().collect() }
    #[inline] pub fn get_resources( &self ) -> &Vec<String> { &self.wit_data.resources }

    #[inline] fn parse_cardinality( _root_path: &PathBuf ) -> Result<InterfaceCardinality, InterfaceParseError> {
        Ok( InterfaceCardinality::ExactlyOne )
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
