
pub struct ResultList<S,E> {
    successful: Vec<S>,
    failed: Vec<E>,
}

impl<S, E> ResultList<S, E> {
    pub fn new() -> Self {
        Self { successful: Vec::new(), failed: Vec::new() }
    }
    pub fn successful( &self ) -> &Vec<S> { &self.successful }
    pub fn failed( &self ) -> &Vec<E> { &self.failed }
    pub fn deconstruct( self ) -> ( Vec<S>, Vec<E> ) {( self.successful, self.failed )}
}

impl<S, E> FromIterator<Result<S, E>> for ResultList<S, E> {
    fn from_iter<T: IntoIterator<Item = Result<S, E>>>(iter: T) -> Self {
        iter.into_iter().fold( ResultList::new(), |mut acc, item| {
            match item {
                Ok(s) => acc.successful.push( s ),
                Err(e) => acc.failed.push( e ),
            }
            acc
        })
    }
}