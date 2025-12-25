
/// Appends an element to the end of list and returns the list
/// Why is this not in std?
pub trait Merge<T> {
    fn merge( self, value: T ) -> Self ;
    fn merge_all( self, values: impl IntoIterator<Item = T> ) -> Self ;
}

impl<T> Merge<T> for Vec<T> {
    fn merge( mut self, value: T ) -> Self {
        self.push( value );
        self
    }
    fn merge_all( mut self, values: impl IntoIterator<Item = T> ) -> Self {
        self.extend( values );
        self
    }
}