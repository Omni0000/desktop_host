#[macro_export]
macro_rules! fixtures {

	{
		bindings = {};
		plugins  = {};
	} => ( mod fixtures {});

	{
		bindings = { $($iname:ident : $ipath:literal),+ $(,)? };
		plugins  = {};
	} => ( mod fixtures {
		fixtures!( @bindings $($iname : $ipath),* );
	});

	{
		bindings = {};
		plugins  = { $($pname:ident : $ppath:literal),+ $(,)? };
	} => ( mod fixtures {
		fixtures!( @plugins $($pname : $ppath),* );
	});

	{
		bindings = { $($iname:ident : $ipath:literal),+ $(,)? };
		plugins  = { $($pname:ident : $ppath:literal),+ $(,)? };
	} => ( mod fixtures {
		fixtures!( @bindings $($iname : $ipath),* );
		fixtures!( @plugins $($pname : $ppath),* );
	});

	( @bindings $($iname:ident : $ipath:literal),+ $(,)? ) => {
		#[allow( dead_code )]
		pub struct Bindings {
			$( pub $iname: $crate::fixture_linking::BindingData, )*
		}
		#[allow( dead_code )]
		pub fn bindings() -> Bindings {
			Bindings {
			$( $iname: $crate::fixture_linking::parse_binding( $crate::fixture_linking::strip_rs( file!() ), $ipath )
				.expect( &format!( "Binding {} failed to initialise", $ipath )), )*
			}
		}
	};

	( @plugins $($pname:ident : $ppath:literal),+ $(,)? ) => {
		#[allow( dead_code )]
		pub struct Plugins {
			$( pub $pname: $crate::fixture_linking::PluginData, )*
		}
		#[allow( dead_code )]
		pub fn plugins( engine: &wasm_link::Engine ) -> Plugins {
			Plugins {
			$( $pname: $crate::fixture_linking::parse_plugin( $crate::fixture_linking::strip_rs( file!() ), $ppath, engine )
				.expect( &format!( "Plugin {} failed to initialise", $ppath )), )*
			}
		}
	};

}

mod fixture_linking {

	use std::collections::{ HashMap, HashSet };
	use wasm_link::{ Component, Engine, Interface, Function, FunctionKind, Plugin };

	pub const fn strip_rs( path: &'static str ) -> &'static str {
		match path.as_bytes() {
			[rest @ .., b'.', b'r', b's'] => {
				// SAFETY: we just checked that the last three bytes are ".rs",
				// so the split is at a UTF-8 boundary.
				unsafe { core::str::from_utf8_unchecked( rest ) }
			}
			_ => unreachable!(),
		}
	}

	#[derive( Debug, thiserror::Error )]
	pub enum FixtureError {
		#[error( "IO error: {0}" )] Io( #[from] std::io::Error ),
		#[error( "WIT parser error: {0}" )] WitParser( String ),
		#[error( "No root interface found" )] NoRootInterface,
		#[error( "No package for root interface" )] NoPackage,
		#[error( "Undeclared type: {0:?}" )] UndeclaredType( wit_parser::TypeId ),
		#[error( "WASM load error: {0}" )] WasmLoad( String ),
	}

	/// Test context that implements `PluginContext`.
	#[derive( Debug )]
	pub struct TestContext {
		pub resource_table: wasm_link::ResourceTable,
	}

	impl wasm_link::PluginContext for TestContext {
		fn resource_table( &mut self ) -> &mut wasm_link::ResourceTable {
			&mut self.resource_table
		}
	}

	/// Parsed binding data from fixtures.
	#[allow( dead_code )]
	pub struct BindingData {
		/// The WIT package name (e.g., "test:primitive")
		pub package: String,
		/// The interface name (e.g., "root")
		pub name: String,
		/// The parsed interface with functions and resources
		pub spec: Interface,
	}

	/// Parsed plugin data from fixtures.
	#[allow( dead_code )]
	pub struct PluginData {
		/// The Plugin ready to link
		pub plugin: Plugin<TestContext>,
	}

	pub fn parse_binding( fixtures_dir: &'static str, id: &str ) -> Result<BindingData, FixtureError> {

		let root_path = std::path::PathBuf::from( fixtures_dir ).join( "bindings" ).join( id );
		let wit_data = parse_wit( &root_path )?;

		Ok( BindingData {
			package: wit_data.package,
			name: wit_data.name,
			spec: Interface::new( wit_data.functions, wit_data.resources ),
		})

	}

	pub fn parse_plugin(
		fixtures_dir: &'static str,
		id: &str,
		engine: &Engine,
	) -> Result<PluginData, FixtureError> {

		let root_path = std::path::PathBuf::from( fixtures_dir ).join( "plugins" ).join( id );

		let wasm_path = root_path.join( "root.wasm" );
		let wasm_path = if wasm_path.exists() { wasm_path } else { root_path.join( "root.wat" ) };

		let component = Component::from_file( engine, &wasm_path )
			.map_err(| e | FixtureError::WasmLoad( format!( "{e:#}" )))?;

		Ok( PluginData {
			plugin: Plugin::new(
				component,
				TestContext { resource_table: wasm_link::ResourceTable::new() },
			),
		})

	}

	struct BindingWitData {
		package: String,
		name: String,
		functions: HashMap<String, Function>,
		resources: HashSet<String>,
	}

	fn parse_wit( root_path: &std::path::Path ) -> Result<BindingWitData, FixtureError> {

		let mut resolve = wit_parser::Resolve::new();
		let _ = resolve.push_path( root_path ).map_err(| err | FixtureError::WitParser( err.to_string() ))?;

		let interface = resolve.interfaces.iter().find(|( _, interface )| match &interface.name {
			Some( name ) => name.as_str() == "root",
			Option::None => false,
		}).ok_or( FixtureError::NoRootInterface )?.1;

		let package = resolve.packages
			.get( interface.package.ok_or( FixtureError::NoPackage )? )
			.ok_or( FixtureError::NoPackage )?
			.name.to_string();

		let functions = interface.functions.iter()
			.map(|( _, function )| {
				let kind = match function.kind {
					wit_parser::FunctionKind::Freestanding
					| wit_parser::FunctionKind::AsyncFreestanding
					| wit_parser::FunctionKind::Static( _ )
					| wit_parser::FunctionKind::AsyncStatic( _ )
					| wit_parser::FunctionKind::Constructor( _ ) => FunctionKind::Freestanding,
					wit_parser::FunctionKind::Method( _ )
					| wit_parser::FunctionKind::AsyncMethod( _ ) => FunctionKind::Method,
				};
				let return_kind = parse_return_kind( &resolve, function.result )?;
				let metadata = Function::new( kind, return_kind );
				Ok(( function.name.clone(), metadata ))
			})
			.collect::<Result<HashMap<_, _>,FixtureError>>()?;

		let resources = interface.types.iter().filter_map(|( name, wit_type_id )| match resolve.types.get( *wit_type_id ) {
			Option::None => Some( Err( FixtureError::UndeclaredType( *wit_type_id ) )),
			Some( wit_type ) if wit_type.kind == wit_parser::TypeDefKind::Resource => Some( Ok( name.clone() )),
			_ => None,
		}).collect::<Result<_, FixtureError>>()?;

		let name = interface.name.clone().ok_or( FixtureError::NoRootInterface )?;

		Ok( BindingWitData { package, name, functions, resources })

	}

	fn parse_return_kind(
		resolve: &wit_parser::Resolve,
		result: Option<wit_parser::Type>
	) -> Result<wasm_link::ReturnKind, FixtureError> {
		let Some( return_type ) = result else { return Ok( wasm_link::ReturnKind::Void )};
		Ok( match has_resource( resolve, return_type )? {
			false => wasm_link::ReturnKind::AssumeNoResources,
			true => wasm_link::ReturnKind::MayContainResources,
		})
	}

	fn has_resource( resolve: &wit_parser::Resolve, wit_type: wit_parser::Type ) -> Result<bool, FixtureError> {
		Ok( match wit_type {
			wit_parser::Type::Id( id ) => match &resolve.types.get( id )
				.ok_or_else(|| FixtureError::UndeclaredType( id ))?
				.kind
			{
				wit_parser::TypeDefKind::Resource
				| wit_parser::TypeDefKind::Handle( wit_parser::Handle::Own( _ )) => true,

				wit_parser::TypeDefKind::Handle( wit_parser::Handle::Borrow( _ ))
				| wit_parser::TypeDefKind::Flags( _ )
				| wit_parser::TypeDefKind::Enum( _ )
				| wit_parser::TypeDefKind::Future( Option::None )
				| wit_parser::TypeDefKind::Stream( Option::None )
				| wit_parser::TypeDefKind::Unknown => false,

				wit_parser::TypeDefKind::Option( wit_type )
				| wit_parser::TypeDefKind::List( wit_type )
				| wit_parser::TypeDefKind::FixedLengthList( wit_type, _ )
				| wit_parser::TypeDefKind::Future( Some( wit_type ))
				| wit_parser::TypeDefKind::Stream( Some( wit_type ))
				| wit_parser::TypeDefKind::Type( wit_type ) => has_resource( resolve, *wit_type )?,

				wit_parser::TypeDefKind::Map( key_type, value_type ) =>
					has_resource( resolve, *key_type )?
					|| has_resource( resolve, *value_type )?,

				wit_parser::TypeDefKind::Result( result ) =>
					( match result.ok { Some( wit_type ) => has_resource( resolve, wit_type )?, _ => false, })
					|| match result.err { Some( wit_type ) => has_resource( resolve, wit_type )?, _ => false, },

				wit_parser::TypeDefKind::Record( record ) => record.fields.iter().try_fold( false, | acc, field |
					Result::<_, FixtureError>::Ok( acc || has_resource( resolve, field.ty )? )
				)?,

				wit_parser::TypeDefKind::Tuple( tuple ) => tuple.types.iter().try_fold( false, | acc, &item |
					Result::<_, FixtureError>::Ok( acc || has_resource( resolve, item )? )
				)?,

				wit_parser::TypeDefKind::Variant( variant ) => variant.cases.iter().try_fold( false, | acc, case |
					Result::<_, FixtureError>::Ok( acc || match case.ty {
						Some( wit_type ) => has_resource( resolve, wit_type )?,
						Option::None => false,
					})
				)?,
			},
			_ => false,
		})
	}
}
