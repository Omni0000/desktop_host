
/// TEMP util until proper warning handling is implemented
pub fn produce_warning<T: std::fmt::Display>( message: T ) {
    println!( "Warning: {}", message );
}