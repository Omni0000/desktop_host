//! Cardinality wrappers for plugin collections.

use std::collections::HashMap ;
use std::future::Future ;
use std::hash::Hash ;

use futures::future::{ BoxFuture, join_all };
use nonempty_collections::{ NEMap, NonEmptyIterator, IntoNonEmptyIterator };
use wasmtime::component::Val ;



/// Cardinality behavior for plugin containers.
///
/// Implementations preserve the container shape while allowing the inner value
/// type to be transformed.
pub trait Cardinality<Id, T>: Sized {
	/// Same cardinality with a different inner type.
	type Rebind<U>;

	/// Maps values by reference while preserving cardinality.
	fn map<N>( &self, map: impl FnMut( &Id, &T ) -> N ) -> Self::Rebind<N>
	where
		Id: Clone;

	/// Maps values by value while preserving cardinality.
	fn map_mut<N>( self, map: impl FnMut( T ) -> N ) -> Self::Rebind<N> ;

	/// Maps cloned values asynchronously while preserving cardinality.
	///
	/// Calls for collection cardinalities may run concurrently.
	///
	/// # Example
	///
	/// ```
	/// use wasm_link::cardinality::{ Cardinality, ExactlyOne };
	///
	/// # futures::executor::block_on( async {
	/// let values = ExactlyOne( "plugin", 41_u32 );
	/// let mapped = values.map_async(| _id, value | async move { value + 1 }).await;
	/// assert_eq!( mapped.1, 42 );
	/// # });
	/// ```
	fn map_async<'a, N, F, Fut>( &'a self, map: F ) -> BoxFuture<'a, Self::Rebind<N>>
	where
		Id: Clone + Send + 'a,
		T: Clone + Send + 'a,
		N: Send + 'a,
		F: Fn( Id, T ) -> Fut + Send + 'a,
		Fut: Future<Output = N> + Send + 'a;

	/// Returns the value associated with `id`, if present.
	fn get( &self, id: &Id ) -> Option<&T>
	where
		Id: Hash + Eq ;
}

/// Exactly one value with ID, guaranteed present.
#[derive( Debug, Clone )]
pub struct ExactlyOne<Id, T>(
	/// The plugin identifier.
	pub Id,
	/// The associated value.
	pub T
);

/// Zero or one value with ID.
#[derive( Debug, Clone )]
pub struct AtMostOne<Id, T>(
	/// Optional `(id, value)` pair.
	pub Option<( Id, T )>
);

/// One or more values keyed by ID.
#[derive( Debug, Clone )]
pub struct AtLeastOne<Id, T>(
	/// Non-empty map keyed by plugin id.
	pub NEMap<Id, T>
);

/// Zero or more values keyed by ID.
#[derive( Debug, Clone )]
pub struct Any<Id, T>(
	/// Map keyed by plugin id.
	pub HashMap<Id, T>
);

impl<Id, T> Cardinality<Id, T> for ExactlyOne<Id, T> {
	type Rebind<U> = ExactlyOne<Id, U>;

	fn map<N>( &self, mut map: impl FnMut( &Id, &T ) -> N ) -> Self::Rebind<N>
	where
		Id: Clone,
	{
		ExactlyOne( self.0.clone(), map( &self.0, &self.1 ))
	}

	fn map_mut<N>( self, mut map: impl FnMut( T ) -> N ) -> Self::Rebind<N> {
		ExactlyOne( self.0, map( self.1 ))
	}

	fn map_async<'a, N, F, Fut>( &'a self, map: F ) -> BoxFuture<'a, Self::Rebind<N>>
	where
		Id: Clone + Send + 'a,
		T: Clone + Send + 'a,
		N: Send + 'a,
		F: Fn( Id, T ) -> Fut + Send + 'a,
		Fut: Future<Output = N> + Send + 'a,
	{
		let id = self.0.clone();
		let value = self.1.clone();
		Box::pin( async move {
			let mapped = map( id.clone(), value ).await;
			ExactlyOne( id, mapped )
		})
	}

	fn get( &self, id: &Id ) -> Option<&T>
	where
		Id: Hash + Eq,
	{
		// In a singleton wrapper, mismatched ids indicate a logic bug upstream.
		// We still return the only value in release builds to avoid masking state.
		debug_assert!( &self.0 == id, "singleton cardinality id mismatch" );
		Some( &self.1 )
	}
}

impl<Id, T> Cardinality<Id, T> for AtMostOne<Id, T> {
	type Rebind<U> = AtMostOne<Id, U>;

	fn map<N>( &self, mut map: impl FnMut( &Id, &T ) -> N ) -> Self::Rebind<N>
	where
		Id: Clone,
	{
		match &self.0 {
			None => AtMostOne( None ),
			Some(( id, value )) => AtMostOne( Some(( id.clone(), map( id, value )))),
		}
	}

	fn map_mut<N>( self, mut map: impl FnMut( T ) -> N ) -> Self::Rebind<N> {
		match self.0 {
			None => AtMostOne( None ),
			Some(( id, value )) => AtMostOne( Some(( id, map( value )))),
		}
	}

	fn map_async<'a, N, F, Fut>( &'a self, map: F ) -> BoxFuture<'a, Self::Rebind<N>>
	where
		Id: Clone + Send + 'a,
		T: Clone + Send + 'a,
		N: Send + 'a,
		F: Fn( Id, T ) -> Fut + Send + 'a,
		Fut: Future<Output = N> + Send + 'a,
	{
		let value = self.0.clone();
		Box::pin( async move { match value {
			None => AtMostOne( None ),
			Some(( id, value )) => {
				let mapped = map( id.clone(), value ).await;
				AtMostOne( Some(( id, mapped )))
			}
		}})
	}

	fn get( &self, id: &Id ) -> Option<&T>
	where
		Id: Hash + Eq,
	{
		match self.0.as_ref() {
			None => None,
			Some(( stored_id, value )) => {
				// In a singleton wrapper, mismatched ids indicate a logic bug upstream.
				// We still return the only value in release builds to avoid masking state.
				debug_assert!( stored_id == id, "singleton cardinality id mismatch" );
				Some( value )
			}
		}
	}
}

impl<Id: Hash + Eq, T> Cardinality<Id, T> for AtLeastOne<Id, T> {
	type Rebind<U> = AtLeastOne<Id, U>;

	fn map<N>( &self, mut map: impl FnMut( &Id, &T ) -> N ) -> Self::Rebind<N>
	where
		Id: Clone,
	{
		AtLeastOne(
			self.0.nonempty_iter()
				.map(|( id, value )| ( id.clone(), map( id, value )))
				.collect()
		)
	}

	fn map_mut<N>( self, mut map: impl FnMut( T ) -> N ) -> Self::Rebind<N> {
		AtLeastOne(
			self.0.into_nonempty_iter()
				.map(|( id, value )| ( id, map( value )))
				.collect()
		)
	}

	fn map_async<'a, N, F, Fut>( &'a self, map: F ) -> BoxFuture<'a, Self::Rebind<N>>
	where
		Id: Clone + Send + 'a,
		T: Clone + Send + 'a,
		N: Send + 'a,
		F: Fn( Id, T ) -> Fut + Send + 'a,
		Fut: Future<Output = N> + Send + 'a,
	{
		let entries = self.0.nonempty_iter()
			.map(|( id, value )| ( id.clone(), value.clone() ))
			.collect::<Vec<_>>();
		Box::pin( async move {
			let mut mapped_values = join_all( entries.into_iter().map(|( id, value )| {
				let future = map( id.clone(), value );
				async move { ( id, future.await ) }
			})).await.into_iter();
			let Some(( first_id, first_mapped )) = mapped_values.next() else { unreachable!() };
			let mut mapped = NEMap::new( first_id, first_mapped );
			mapped.extend( mapped_values );
			AtLeastOne( mapped )
		})
	}

	fn get( &self, id: &Id ) -> Option<&T>
	where
		Id: Hash + Eq,
	{
		self.0.get( id )
	}
}

impl<Id: Hash + Eq, T> Cardinality<Id, T> for Any<Id, T> {
	type Rebind<U> = Any<Id, U>;

	fn map<N>( &self, mut map: impl FnMut( &Id, &T ) -> N ) -> Self::Rebind<N>
	where
		Id: Clone,
	{
		Any( self.0.iter().map(|( id, value )| ( id.clone(), map( id, value ))).collect() )
	}

	fn map_mut<N>( self, mut map: impl FnMut( T ) -> N ) -> Self::Rebind<N> {
		Any( self.0.into_iter().map(|( id, value )| ( id, map( value ))).collect() )
	}

	fn map_async<'a, N, F, Fut>( &'a self, map: F ) -> BoxFuture<'a, Self::Rebind<N>>
	where
		Id: Clone + Send + 'a,
		T: Clone + Send + 'a,
		N: Send + 'a,
		F: Fn( Id, T ) -> Fut + Send + 'a,
		Fut: Future<Output = N> + Send + 'a,
	{
		let entries = self.0.iter().map(|( id, value )| ( id.clone(), value.clone() )).collect::<Vec<_>>();
		Box::pin( async move {
			Any( join_all( entries.into_iter().map(|( id, value )| {
				let future = map( id.clone(), value );
				async move { ( id, future.await ) }
			})).await.into_iter().collect() )
		})
	}

	fn get( &self, id: &Id ) -> Option<&T>
	where
		Id: Hash + Eq,
	{
		self.0.get( id )
	}
}

impl<Id: Hash + Eq + Into<Val>> From<ExactlyOne<Id, Val>> for Val {
	fn from( socket: ExactlyOne<Id, Val> ) -> Self {
		Val::Tuple( vec![ socket.0.into(), socket.1 ])
	}
}

impl<Id: Hash + Eq + Into<Val>> From<AtMostOne<Id, Val>> for Val {
	fn from( socket: AtMostOne<Id, Val> ) -> Self {
		match socket.0 {
			None => Val::Option( None ),
			Some(( id, val )) => Val::Option( Some( Box::new( Val::Tuple( vec![ id.into(), val ] )))),
		}
	}
}

impl<Id: Hash + Eq + Into<Val>> From<AtLeastOne<Id, Val>> for Val {
	fn from( socket: AtLeastOne<Id, Val> ) -> Self {
		Val::Map(
			socket.0.into_iter()
				.map(|( id, val )| ( id.into(), val ))
				.collect()
		)
	}
}

impl<Id: Hash + Eq + Into<Val>> From<Any<Id, Val>> for Val {
	fn from( socket: Any<Id, Val> ) -> Self {
		Val::Map(
			socket.0.into_iter()
				.map(|( id, val )| ( id.into(), val ))
				.collect()
		)
	}
}
