use std::collections::HashMap ;
use std::sync::{ RwLock, RwLockReadGuard, PoisonError };
use wasmtime::component::Val ;

use crate::PluginId ;
use super::PluginInstance ;



#[derive( Debug )]
pub enum Socket<T> {
    AtMostOne( Option<T> ),
    ExactlyOne( T ),
    AtLeastOne( HashMap<PluginId, T> ),
    Any( HashMap<PluginId, T> ),
}

impl<T> Socket<T> {
    pub fn map<N>( &self, mut map: impl FnMut(&T) -> N ) -> Socket<N> {
        match self {
            Self::AtMostOne( Option::None ) => Socket::AtMostOne( Option::None ),
            Self::AtMostOne( Some( t )) => Socket::AtMostOne( Some( map( t ))),
            Self::ExactlyOne( t ) => Socket::ExactlyOne( map( t )),
            Self::AtLeastOne( vec ) => Socket::AtLeastOne( vec.iter().map(|( id, item )| ( id.clone(), map( item ) )).collect() ),
            Self::Any( vec ) => Socket::Any( vec.iter().map(|( id, item )| ( id.clone(), map( item ) )).collect() ),
        }
    }
    pub fn map_mut<N>( self, mut map: impl FnMut(T) -> N ) -> Socket<N> {
        match self {
            Self::AtMostOne( Option::None ) => Socket::AtMostOne( Option::None ),
            Self::AtMostOne( Some( t )) => Socket::AtMostOne( Some( map( t ))),
            Self::ExactlyOne( t ) => Socket::ExactlyOne( map( t )),
            Self::AtLeastOne( vec ) => Socket::AtLeastOne( vec.into_iter().map(|( id, item )| ( id, map( item ) )).collect() ),
            Self::Any( vec ) => Socket::Any( vec.into_iter().map(|( id, item )| ( id, map( item ))).collect() ),
        }
    }
}
impl Socket<PluginInstance> {
    pub fn get( &self, id: &PluginId ) -> Option<&PluginInstance> {
        match self {
            Self::AtMostOne( Some( plugin )) if plugin.id() == id => Some( plugin ),
            Self::AtMostOne( Option::None ) | Self::AtMostOne( _ ) => None,
            Self::ExactlyOne( plugin ) if plugin.id() == id => Some( plugin ),
            Self::ExactlyOne( _ ) => None,
            Self::AtLeastOne( plugins ) | Self::Any( plugins ) => plugins.get( id ),
        }
    }
}
impl Socket<RwLock<PluginInstance>> {
    pub fn get( &self, id: &PluginId ) -> Result<Option<&RwLock<PluginInstance>>,PoisonError<RwLockReadGuard<'_, PluginInstance>>> {
        Ok( match self {
            Self::AtMostOne( Option::None ) => None,
            Self::AtMostOne( Some( plugin )) | Self::ExactlyOne( plugin ) => {
                if &plugin.read()?.id == id { Some( plugin ) } else { None }
            },
            Self::AtLeastOne( plugins ) | Self::Any( plugins ) => plugins.get( id ),
        })
    }
}

impl Into<Val> for Socket<Val> {
    fn into( self ) -> Val {
        match self {
            Self::AtMostOne( Option::None ) => Val::Option( Option::None ),
            Self::AtMostOne( Some( val )) => Val::Option( Some( Box::new( val ))),
            Self::ExactlyOne( val ) => val,
            Self::AtLeastOne( items )
            | Self::Any( items ) => Val::List( items.into_iter().map(|( _, item )| item ).collect() ),
        }
    }
}
