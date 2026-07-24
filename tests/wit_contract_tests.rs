use std::collections::HashSet ;

use wasm_link::{
	Component, DispatchError, Engine, ResourceCreationError, ResourceReceiveError, Val,
};
use wasmtime::Store ;
use wit_component::{ ComponentEncoder, StringEncoding, dummy_module, embed_component_metadata };
use wit_parser::{ ManglingAndAbi, Resolve, TypeDefKind, WorldItem };



#[test]
fn dispatch_errors_cover_the_provided_wit_contract() -> Result<(), Box<dyn std::error::Error>> {
	let (resolve, world) = load_contract()?;
	let contract_case_count = dispatch_error_case_count( &resolve, world )?;
	let mut module = dummy_module( &resolve, world, ManglingAndAbi::Standard32 );
	embed_component_metadata( &mut module, &resolve, world, StringEncoding::UTF8 )?;
	let component = ComponentEncoder::default().module( &module )?.encode()?;

	let engine = Engine::default();
	let component = Component::new( &engine, component )?;
	let linker = wasmtime::component::Linker::<()>::new( &engine );
	let mut store = Store::new( &engine, () );
	let instance = linker.instantiate( &mut store, &component )?;
	let validate = instance.get_func( &mut store, "validate" ).ok_or( "missing validate export" )?;

	let values = dispatch_error_values();
	let variant_names = values.iter().map( variant_name ).collect::<Result<HashSet<_>, _>>()?;
	assert_eq!( variant_names.len(), values.len(), "dispatch errors must have unique WIT variants" );
	assert_eq!( values.len(), contract_case_count, "every WIT dispatch-error case must be produced" );
	let invalid = Val::Variant( "not-in-contract".to_string(), None );
	let error = validate.call( &mut store, &[ invalid ], &mut [] )
		.expect_err( "an unknown WIT variant must be rejected" );
	assert!( error.downcast_ref::<wasmtime::Trap>().is_none(), "invalid value reached the component implementation" );

	for value in values {
		let error = validate.call( &mut store, &[ value ], &mut [] )
			.expect_err( "the generated validator should trap after accepting its argument" );
		assert!( error.downcast_ref::<wasmtime::Trap>().is_some(), "value was rejected before reaching the WIT ABI" );
	}

	Ok(())
}

fn load_contract() -> Result<(Resolve, wit_parser::WorldId), Box<dyn std::error::Error>> {
	let mut resolve = Resolve::new();
	let _ = resolve.push_path( "wit" )?;
	let (validator, _) = resolve.push_path( "tests/wit_contract" )?;
	let world = resolve.select_world( &[ validator ], Some( "validator" ))?;
	Ok(( resolve, world ))
}

fn dispatch_error_case_count(
	resolve: &Resolve,
	world: wit_parser::WorldId,
) -> Result<usize, Box<dyn std::error::Error>> {
	let parameter = resolve.worlds[world].exports.values().find_map(| item | match item {
		WorldItem::Function( function ) => function.params.first().map(| parameter | parameter.ty ),
		_ => None,
	}).ok_or( "validator has no function parameter" )?;
	variant_case_count( resolve, parameter )
}

fn variant_case_count(
	resolve: &Resolve,
	type_: wit_parser::Type,
) -> Result<usize, Box<dyn std::error::Error>> {
	let wit_parser::Type::Id( id ) = type_ else { return Err( "validator parameter is not a named type".into() )};
	match &resolve.types[id].kind {
		TypeDefKind::Variant( variant ) => Ok( variant.cases.len() ),
		TypeDefKind::Type( alias ) => variant_case_count( resolve, *alias ),
		_ => Err( "validator parameter is not a variant".into() ),
	}
}

fn dispatch_error_values() -> Vec<Val> {
	vec![
		DispatchError::InvalidInterfacePath( "package/interface".to_string() ).into(),
		DispatchError::InvalidFunction( "function".to_string() ).into(),
		DispatchError::MissingResponse.into(),
		DispatchError::RuntimeException( wasmtime::Error::msg( "trap" )).into(),
		DispatchError::InvalidArgumentList.into(),
		DispatchError::UnsupportedType( "future".to_string() ).into(),
		DispatchError::ResourceCreationError( ResourceCreationError::ResourceTableFull ).into(),
		DispatchError::ResourceCreationError( ResourceCreationError::ResourceHandleConversionFailed ).into(),
		DispatchError::ResourceReceiveError( ResourceReceiveError::InvalidHandle ).into(),
	]
}

fn variant_name( value: &Val ) -> Result<&str, &'static str> {
	match value {
		Val::Variant( name, _ ) => Ok( name ),
		_ => Err( "dispatch error did not convert to a variant" ),
	}
}
