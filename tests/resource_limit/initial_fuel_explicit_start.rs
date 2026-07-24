use wasm_link::{ Engine, Linker };
use wasmtime::Config ;

fixtures! {
	bindings = {};
	plugins  = { startup: "startup" };
}

fn engine() -> Engine {
	let mut config = Config::new();
	config.consume_fuel( true );
	Engine::new( &config ).expect( "failed to create engine" )
}

fn instantiate_with_fuel( fuel: u64 ) -> Result<(), wasmtime::Error> {
	let engine = engine();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	plugins.startup.plugin
		.with_initial_fuel( fuel )
		.instantiate( &engine, &linker )?;
	Ok(())
}

async fn instantiate_async_with_fuel( fuel: u64 ) -> Result<(), wasmtime::Error> {
	let engine = engine();
	let linker = Linker::new( &engine );
	let plugins = fixtures::plugins( &engine );
	plugins.startup.plugin
		.with_initial_fuel( fuel )
		.instantiate_async( &engine, &linker ).await?;
	Ok(())
}

#[test]
fn explicit_start_exhausts_insufficient_initial_fuel() {
	assert!( instantiate_with_fuel( 0 ).is_err() );
}

#[test]
fn explicit_start_accepts_sufficient_initial_fuel() {
	instantiate_with_fuel( 100_000 ).expect( "explicit start should have sufficient fuel" );
}

#[test]
fn async_explicit_start_exhausts_insufficient_initial_fuel() {
	futures::executor::block_on( async {
		assert!( instantiate_async_with_fuel( 0 ).await.is_err() );
	});
}

#[test]
fn async_explicit_start_accepts_sufficient_initial_fuel() {
	futures::executor::block_on( async {
		instantiate_async_with_fuel( 100_000 ).await.expect( "async explicit start should have sufficient fuel" );
	});
}
