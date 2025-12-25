
/// Maps iterator elements, passing a single value through. Each iteration should
/// return a new element as well as the updated passthrough. The iterator can then
/// be consumed using one of the custom MapScan methods to extract the passthrough
pub trait MapScanTrait: Iterator + Sized {
    fn map_scan<P, F, R>( self, init: P, f: F ) -> MapScan<Self, P, F>
    where
        F: FnMut( Self::Item, P ) -> ( R, P ),
    {
        MapScan {
            iter: self,
            state: init,
            f,
        }
    }
}

impl<T: Iterator> MapScanTrait for T {}

#[must_use = "iterators are lazy and do nothing unless consumed" ]
pub struct MapScan<I, P, F> {
    iter: I,
    state: P,
    f: F,
}

impl<I, P, F, T, R> MapScan<I, P, F>
where
    I: Iterator<Item = T>,
    F: FnMut( T, P ) -> ( R, P ),
{
    pub fn into_inner( self ) -> P {
        self.state
    }
}

impl<I, P, F, T, L, R> MapScan<I, P, F>
where
    I: Iterator<Item = T>,
    F: FnMut( T, P ) -> (( L, R ), P ),
{
    pub fn unzip_get_state<A, B>( mut self ) -> (( A, B ), P )
    where
        A: FromIterator<L> + Extend<L> + Default,
        B: FromIterator<R> + Extend<R> + Default,
    {
        ( self.by_ref().unzip(), self.into_inner() )
    }
}

impl<I, P, F, T, R> Iterator for MapScan<I, P, F>
where
    I: Iterator<Item = T>,
    F: FnMut( T, P ) -> ( R, P ),
{
    type Item = R;

    fn next( &mut self ) -> Option<Self::Item> {
        let item = self.iter.next()?;
        let mapped = unsafe {
            // UNSAFE: takes self.state, maps it and replaces it with the new value
            // NOTE: UB on self.f panic but who cares about the panic path anyways
            let old_state = std::ptr::read( &mut self.state );
            let ( mapped, new_state ) = ( self.f )( item, old_state );
            std::ptr::write( &mut self.state, new_state );
            mapped
        };
        Some( mapped )
    }

    fn size_hint( &self ) -> ( usize, Option<usize> ) {
        self.iter.size_hint()
    }
}