use wasmtime::{ Config, Engine, Store };
use wasmtime::component::{ Component, FutureReader, Linker, ResourceTable, StreamReader, Val };

use super::{
	CallRequest, CallResponse, CallerToken, DispatchQueue, DispatchSession, InstanceDispatcher,
	PluginInstanceAsync, PluginInstanceSync, SessionBatch, SessionSlot, ensure_supported_value,
};
use crate::{ DispatchError, PluginContext };

struct Context { table: ResourceTable }

impl PluginContext for Context {
	fn resource_table( &mut self ) -> &mut ResourceTable { &mut self.table }
}

#[test]
fn plugin_instances_are_send_and_sync() {
	fn assert_send_sync<T: Send + Sync>() {}
	assert_send_sync::<PluginInstanceSync<Context>>();
	assert_send_sync::<PluginInstanceAsync<Context>>();
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

fn request( name: &str ) -> CallRequest {
	let ( response, _result ) = futures::channel::oneshot::channel();
	CallRequest {
		package_name: "test:queue".to_string(),
		interface_name: "root".to_string(),
		function_name: name.to_string(),
		function: crate::Function::new(
			crate::FunctionKind::Freestanding,
			crate::ReturnKind::AssumeNoResources,
		),
		data: Vec::new(),
		response: CallResponse::new( response ),
	}
}

fn empty_queue() -> DispatchQueue {
	DispatchQueue {
		active: None,
		waiting: std::collections::VecDeque::new(),
		running: false,
		closing: false,
	}
}

fn empty_dispatcher() -> std::sync::Arc<InstanceDispatcher<Context>> {
	std::sync::Arc::new( InstanceDispatcher {
		state: std::sync::Mutex::new( None ),
		queue: std::sync::Mutex::new( empty_queue() ),
		waker: futures::task::AtomicWaker::new(),
		session_slot: SessionSlot::new(),
	})
}

#[test]
fn session_batch_preserves_caller_fifo_and_rotates_between_callers() {
	let session = DispatchSession::new();
	let first = CallerToken::new();
	let second = CallerToken::new();
	let mut batch = SessionBatch::new( &session );
	batch.push( first.clone(), request( "first-1" ));
	batch.push( first, request( "first-2" ));
	batch.push( second, request( "second-1" ));

	assert_eq!( batch.pop().map(| request | request.function_name ), Some( "first-1".to_string() ));
	assert_eq!( batch.pop().map(| request | request.function_name ), Some( "second-1".to_string() ));
	assert_eq!( batch.pop().map(| request | request.function_name ), Some( "first-2".to_string() ));
	assert!( batch.pop().is_none() );
}

#[test]
fn dispatcher_discards_cancelled_sessions_and_groups_waiting_calls() {
	let cancelled = DispatchSession::new();
	let mut cancelled_batch = SessionBatch::new( &cancelled );
	cancelled_batch.push( CallerToken::new(), request( "cancelled" ));
	drop( cancelled );

	let live = DispatchSession::new();
	let mut queue = empty_queue();
	queue.running = true;
	assert!( InstanceDispatcher::<Context>::start_if_idle( &mut queue ).is_none() );
	queue.active = Some( cancelled_batch );
	InstanceDispatcher::<Context>::clear_cancelled_active( &mut queue );
	assert!( !queue.running );
	assert!( queue.active.is_none() );

	let cancelled = DispatchSession::new();
	queue.waiting.push_back( SessionBatch::new( &cancelled ));
	drop( cancelled );
	InstanceDispatcher::<Context>::push_waiting(
		&mut queue.waiting, &live, CallerToken::new(), request( "first" ),
	);
	InstanceDispatcher::<Context>::push_waiting(
		&mut queue.waiting, &live, CallerToken::new(), request( "second" ),
	);
	assert_eq!( queue.waiting.back().map(| batch | batch.callers.len() ), Some( 2 ));
	assert!( InstanceDispatcher::<Context>::start_if_idle( &mut queue ).is_some() );
	assert!( queue.waiting.is_empty() );
}

#[test]
fn closing_dispatcher_defers_new_calls_and_reports_ready() {
	let dispatcher = empty_dispatcher();
	let session = DispatchSession::new();
	{
		let mut queue = super::lock_unpoisoned( &dispatcher.queue );
		queue.closing = true;
		queue.running = true;
		queue.active = Some( SessionBatch::new( &session ));
	}
	dispatcher.submit( &session, CallerToken::new(), request( "deferred" ));
	assert!( dispatcher.pop_request( &session ).is_none() );

	let waker = futures::task::noop_waker();
	let mut context = std::task::Context::from_waker( &waker );
	assert!( dispatcher.poll_request_ready( &session, &mut context ).is_ready() );
}

#[test]
fn missing_state_finishes_a_batch_and_starts_the_next_session() {
	futures::executor::block_on( async {
		let dispatcher = empty_dispatcher();
		let first = DispatchSession::new();
		let second = DispatchSession::new();
		{
			let mut queue = super::lock_unpoisoned( &dispatcher.queue );
			queue.running = true;
			queue.active = Some( SessionBatch::new( &first ));
			let mut waiting = SessionBatch::new( &second );
			waiting.push( CallerToken::new(), request( "next" ));
			queue.waiting.push_back( waiting );
		}
		std::sync::Arc::clone( &dispatcher ).run_batch( first ).await;
		let queue = super::lock_unpoisoned( &dispatcher.queue );
		assert!( queue.running );
		assert!( queue.active.as_ref().is_some_and(| batch | batch.belongs_to( &second )));
	});
}

#[test]
fn unpolled_batch_does_not_retain_its_cancelled_session() {
	let dispatcher = empty_dispatcher();
	let session = DispatchSession::new();
	dispatcher.spawn_batch( &session );
	let future = super::lock_unpoisoned( &session.incoming ).pop()
		.expect( "spawned batch should be queued" );
	drop( session );
	futures::executor::block_on( future );
}

#[test]
fn concurrent_store_errors_reach_admitted_and_queued_calls() {
	futures::executor::block_on( async {
		let dispatcher = empty_dispatcher();
		let session = DispatchSession::new();
		let ( admitted_sender, admitted_result ) = futures::channel::oneshot::channel();
		let admitted = CallResponse::new( admitted_sender );
		let ( queued_sender, queued_result ) = futures::channel::oneshot::channel();
		let mut queued = request( "queued" );
		queued.response = CallResponse::new( queued_sender );
		{
			let mut queue = super::lock_unpoisoned( &dispatcher.queue );
			let mut batch = SessionBatch::new( &session );
			batch.push( CallerToken::new(), queued );
			queue.active = Some( batch );
		}

		dispatcher.finish_concurrent_batch(
			&session,
			vec![ admitted ],
			Err( wasmtime::Error::msg( "store task failed" )),
		);

		for result in [ admitted_result.await, queued_result.await ] {
			assert!( matches!(
				result,
				Ok( Err( DispatchError::RuntimeException( error )))
					if error.to_string() == "store task failed"
			));
		}
	});
}
