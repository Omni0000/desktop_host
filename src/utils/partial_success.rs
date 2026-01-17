/// Type aliases for operations that support partial success/failure patterns.
/// These represent graceful error handling where some parts of an operation may fail
/// while others succeed, allowing partial completion rather than total failure.

/// Represents a successful operation where some parts failed but didn't prevent overall success.
/// The Vec<E> contains errors from the failed parts that were handled gracefully.
pub type PartialSuccess<T, E> = ( T, Vec<E> );

/// Represents an operation that may partially succeed or fail.
/// Ok: Core success data plus errors from partial failures that allowed completion.
/// Err: Primary failure cause plus errors that likely contributed to the overall failure.
pub type PartialResult<T, F, E> = Result<( T, Vec<E> ), ( F, Vec<E> )>;

pub fn deconstruct_partial_result<T, F, E>( result: PartialResult<T, F, E> ) -> ( Result<T, F>, Vec<E> ) {
    match result {
        Ok(( value, errors )) => ( Ok( value ), errors ),
        Err(( failure, errors )) => ( Err( failure ), errors ),
    }
}
