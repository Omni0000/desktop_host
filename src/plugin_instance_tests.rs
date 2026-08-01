use std::future::Future ;
use std::sync::Arc ;

use wasmtime::{ Config, Engine, Store };
use wasmtime::component::{ Component, FutureReader, Linker, ResourceTable, StreamReader, Val };

use super::{
	AsyncInstanceInner, AsyncLinkage, AsyncRequest, AsyncRuntimeError, AttachedDriver, DriverMessage,
	DriverState, PluginInstanceAsync, PluginState, ensure_supported_value, receive_response,
};
use crate::async_scheduler::{ ActiveCalls, AsyncScheduler, DispatchContext, PluginKey };
use crate::{ DispatchError, Function, FunctionKind, PluginContext, ReturnKind };

struct Context { table: ResourceTable }

impl PluginContext for Context {
	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table }
}

#[test]
fn dropping_a_request_reports_the_exact_cancellation_error() -> Result<(), Box<dyn std::error::Error>> {
	let ( request, result ) = request( "get-value" );
	drop( request );
	assert_runtime_error( futures::executor::block_on( result )?, &AsyncRuntimeError::CallCancelled )?;
	Ok(())
}

#[test]
fn dropped_response_sender_reports_the_exact_missing_response_error() -> Result<(), Box<dyn std::error::Error>> {
	let ( response, result ) = futures::channel::oneshot::channel();
	drop( response );
	assert_runtime_error(
		futures::executor::block_on( receive_response( result )),
		&AsyncRuntimeError::MissingResponse,
	)
}

#[test]
fn failed_driver_rejects_later_calls_with_the_exact_error() -> Result<(), Box<dyn std::error::Error>> {
	let expected = AsyncRuntimeError::StoreFailed( "expected".to_string() );
	let ( sender, _receiver ) = futures::channel::mpsc::unbounded();
	let inner = native_inner( sender, DriverState::Failed( expected.clone() ));
	let ( request, result ) = request( "get-value" );
	inner.enqueue( &AsyncScheduler::testing(), request );
	assert_runtime_error( futures::executor::block_on( result )?, &expected )
}

#[test]
fn stopped_driver_rejects_the_request_with_the_exact_error() -> Result<(), Box<dyn std::error::Error>> {
	let ( sender, receiver ) = futures::channel::mpsc::unbounded();
	drop( receiver );
	let inner = native_inner( sender, DriverState::Idle( Box::pin( futures::future::pending() )));
	let ( request, result ) = request( "get-value" );
	inner.enqueue( &AsyncScheduler::testing(), request );
	assert_runtime_error( futures::executor::block_on( result )?, &AsyncRuntimeError::DriverStopped )
}

#[test]
fn closed_scheduler_rejects_a_call_with_the_exact_cancellation_error() -> Result<(), Box<dyn std::error::Error>> {
	let state = test_state_without_concurrency()?;
	let plugin = PluginInstanceAsync::from( super::PluginInstanceSync {
		state,
	});
	let scheduler = AsyncScheduler::testing();
	let path = crate::async_scheduler::ExecutionPathId::ROOT;
	scheduler.close();
	let ( request, result ) = request( "get-value" );
	scheduler.schedule( scheduler.origin(), path, plugin.handle(), request );
	assert_runtime_error( futures::executor::block_on( result )?, &AsyncRuntimeError::CallCancelled )
}

#[test]
fn attached_driver_records_its_exact_terminal_error() {
	let expected = AsyncRuntimeError::StoreFailed( "expected".to_string() );
	let ( sender, _receiver ) = futures::channel::mpsc::unbounded();
	let inner = native_inner( sender, DriverState::Attached { owner: 0, waiting: std::collections::VecDeque::new() });
	let mut driver = Box::pin( AttachedDriver {
		inner: Arc::clone( &inner ),
		future: Some( Box::pin( futures::future::ready( expected.clone() ))),
	});
	let waker = futures::task::noop_waker();
	let mut context = std::task::Context::from_waker( &waker );
	assert!( driver.as_mut().poll( &mut context ).is_ready() );
	assert!( driver.as_mut().poll( &mut context ).is_ready() );
	let state = inner.driver.lock().unwrap_or_else( std::sync::PoisonError::into_inner );
	assert!( matches!( &*state, DriverState::Failed( error ) if error == &expected ));
}

#[test]
fn store_failure_reaches_the_call_as_the_exact_runtime_error() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let PluginState { store, instance, .. } = test_state_without_concurrency()?;
		let plugin = PluginInstanceAsync::new(
			store,
			instance,
			std::collections::HashMap::new(),
			AsyncLinkage::new(),
			None,
			None,
		);
		let expected = AsyncRuntimeError::StoreFailed(
			"cannot use `run_concurrent` when Config::concurrency_support disabled".to_string(),
		);
		let result = crate::async_scheduler::run( move | scheduler | async move {
			let path = crate::async_scheduler::ExecutionPathId::ROOT;
			plugin.handle().dispatch_async(
				DispatchContext::new( &scheduler, scheduler.origin(), path ),
				"test:single-plugin",
				"root",
				"get-value",
				&Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
				&[],
			).await
		}).await;
		assert_runtime_error( result, &expected )
	})
}

#[test]
fn concurrent_driver_finishes_an_admitted_call_before_shutdown() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut state = test_state().await?;
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		let ( request, result ) = request( "get-value" );
		sender.unbounded_send( DriverMessage::Call( request ))?;
		drop( sender );
		assert_eq!( state.run_requests( receiver, active_calls() ).await, AsyncRuntimeError::DriverStopped );
		assert!( matches!( result.await, Ok( Ok( Val::U32( 42 )))));
		Ok(())
	})
}

#[test]
fn concurrent_driver_reports_exact_invalid_export_errors() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut config = Config::new();
		config.concurrency_support( true );
		let engine = Engine::new( &config )?;
		let component = Component::from_file(
			&engine,
			concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatch_error/invalid_function/plugins/test-plugin/root.wat" ),
		)?;
		let linker = Linker::<Context>::new( &engine );
		let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
		let instance = linker.instantiate_async( &mut store, &component ).await?;
		let mut state = PluginState {
			store,
			instance,
			interface_remaps: std::collections::HashMap::new(),
			fuel_limiter: None,
			epoch_limiter: None,
		};
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		let ( mut missing_interface, missing_interface_result ) = request( "test" );
		missing_interface.package_name = "test:dispatch-error".to_string();
		missing_interface.interface_name = "missing".to_string();
		let ( mut non_function, non_function_result ) = request( "not-a-function" );
		non_function.package_name = "test:dispatch-error".to_string();
		sender.unbounded_send( DriverMessage::Call( missing_interface ))?;
		sender.unbounded_send( DriverMessage::Call( non_function ))?;
		drop( sender );

		assert_eq!( state.run_requests( receiver, active_calls() ).await, AsyncRuntimeError::DriverStopped );
		assert!( matches!(
			missing_interface_result.await?,
			Err( DispatchError::InvalidInterfacePath( path )) if path == "test:dispatch-error/missing"
		));
		assert!( matches!(
			non_function_result.await?,
			Err( DispatchError::InvalidFunction( function )) if function == "test:dispatch-error/root:not-a-function"
		));
		Ok(())
	})
}

#[test]
fn limited_driver_queues_calls_and_finishes_them_before_shutdown() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut state = test_sync_state().await?;
		state.epoch_limiter = Some( Box::new(| _, _, _, _ | u64::MAX ));
		let ( sender, receiver ) = futures::channel::mpsc::unbounded();
		let ( first, first_result ) = blocking_request( "get-primitive" );
		let ( second, second_result ) = blocking_request( "get-primitive" );
		sender.unbounded_send( DriverMessage::Call( first ))?;
		sender.unbounded_send( DriverMessage::Call( second ))?;
		drop( sender );
		assert_eq!( state.run_requests( receiver, active_calls() ).await, AsyncRuntimeError::RequestChannelClosed );
		let first = first_result.await?;
		let second = second_result.await?;
		assert!( matches!( first, Ok( Val::U32( 42 ))), "unexpected first result: {first:?}" );
		assert!( matches!( second, Ok( Val::U32( 42 ))), "unexpected second result: {second:?}" );
		Ok(())
	})
}

fn native_inner(
	sender: futures::channel::mpsc::UnboundedSender<DriverMessage<Context>>,
	driver: DriverState<Context>,
) -> Arc<AsyncInstanceInner<Context>> {
	Arc::new( AsyncInstanceInner {
		sender,
		driver: std::sync::Mutex::new( driver ),
	})
}

fn request(
	name: &str,
) -> (
	AsyncRequest<Context>,
	futures::channel::oneshot::Receiver<Result<Val, DispatchError>>,
) {
	let ( response, result ) = futures::channel::oneshot::channel();
	let scheduler = AsyncScheduler::testing();
	let path = crate::async_scheduler::ExecutionPathId::ROOT;
	( AsyncRequest {
		package_name: "test:single-async".to_string(),
		interface_name: "root".to_string(),
		function_name: name.to_string(),
		function: Function::new( FunctionKind::Freestanding, ReturnKind::AssumeNoResources ),
		data: Vec::new(),
		response: Some( response ),
		active: crate::async_scheduler::ActiveCall { scheduler, caller: PluginKey( 1 ), path },
	}, result )
}

fn blocking_request(
	name: &str,
) -> (
	AsyncRequest<Context>,
	futures::channel::oneshot::Receiver<Result<Val, DispatchError>>,
) {
	let ( mut request, result ) = request( name );
	request.package_name = "test:primitive".to_string();
	( request, result )
}

fn active_calls() -> ActiveCalls<Context> {
	Arc::new( std::sync::Mutex::new( std::collections::HashMap::new() ))
}

fn assert_runtime_error(
	result: Result<Val, DispatchError>,
	expected: &AsyncRuntimeError,
) -> Result<(), Box<dyn std::error::Error>> {
	let Err( DispatchError::RuntimeException( error )) = result else {
		return Err( format!( "expected RuntimeException, found {result:?}" ).into() );
	};
	assert_eq!( error.downcast_ref::<AsyncRuntimeError>(), Some( expected ));
	Ok(())
}

async fn test_state() -> Result<PluginState<Context>, Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatching/single_plugin_async/plugins/plugin/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate_async( &mut store, &component ).await?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

async fn test_sync_state() -> Result<PluginState<Context>, Box<dyn std::error::Error>> {
	let engine = Engine::default();
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatching/single_plugin_expect_primitive/plugins/get-value/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate_async( &mut store, &component ).await?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

fn test_state_without_concurrency() -> Result<PluginState<Context>, wasmtime::Error> {
	let mut config = Config::new();
	config.concurrency_support( false );
	let engine = Engine::new( &config )?;
	let component = Component::from_file(
		&engine,
		concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/dispatching/single_plugin_expect_primitive/plugins/get-value/root.wat" ),
	)?;
	let linker = Linker::<Context>::new( &engine );
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let instance = linker.instantiate( &mut store, &component )?;
	Ok( PluginState {
		store,
		instance,
		interface_remaps: std::collections::HashMap::new(),
		fuel_limiter: None,
		epoch_limiter: None,
	})
}

#[test]
fn accepts_nested_component_values() -> Result<(), DispatchError> {
	let value = Val::Record( vec![
		( "list".to_string(), Val::List( vec![ Val::U32( 1 ) ])),
		( "tuple".to_string(), Val::Tuple( vec![ Val::U32( 2 ) ])),
		( "map".to_string(), Val::Map( vec![( Val::String( "key".to_string() ), Val::U32( 3 ))])),
		( "variant".to_string(), Val::Variant( "some".to_string(), Some( Box::new( Val::U32( 4 ))))),
		( "option".to_string(), Val::Option( Some( Box::new( Val::U32( 5 ))))),
		( "ok".to_string(), Val::Result( Ok( Some( Box::new( Val::U32( 6 )))))),
		( "err".to_string(), Val::Result( Err( Some( Box::new( Val::U32( 7 )))))),
	]);
	ensure_supported_value( &value )
}

#[test]
fn rejects_future_and_stream_values() -> Result<(), Box<dyn std::error::Error>> {
	let mut config = Config::new();
	config.concurrency_support( true );
	let engine = Engine::new( &config )?;
	let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
	let future = FutureReader::new( &mut store, async { Ok::<_, wasmtime::Error>( 1_u32 )})?
		.try_into_future_any( &mut store )?;
	let stream = StreamReader::new( &mut store, vec![ 1_u32 ])?
		.try_into_stream_any( &mut store )?;

	assert!( matches!(
		ensure_supported_value( &Val::Future( future )),
		Err( DispatchError::UnsupportedType( name )) if name == "future"
	));
	assert!( matches!(
		ensure_supported_value( &Val::Stream( stream )),
		Err( DispatchError::UnsupportedType( name )) if name == "stream"
	));
	Ok(())
}

#[test]
fn rejects_error_context_values() -> Result<(), Box<dyn std::error::Error>> {
	futures::executor::block_on( async {
		let mut config = Config::new();
		config.wasm_component_model_async( true );
		config.wasm_component_model_error_context( true );
		let engine = Engine::new( &config )?;
		let component = Component::from_file(
			&engine,
			concat!( env!( "CARGO_MANIFEST_DIR" ), "/tests/plugin_instance/error_context.wat" ),
		)?;
		let linker = Linker::<Context>::new( &engine );
		let mut store = Store::new( &engine, Context { table: ResourceTable::new() });
		let instance = linker.instantiate_async( &mut store, &component ).await?;
		let function = instance.get_func( &mut store, "make-error-context" )
			.ok_or( "missing make-error-context export" )?;
		let mut results = [ Val::Bool( false ) ];
		function.call_async( &mut store, &[], &mut results ).await?;
		assert!( matches!(
			ensure_supported_value( &results[0] ),
			Err( DispatchError::UnsupportedType( name )) if name == "error-context"
		));
		Ok(())
	})
}
