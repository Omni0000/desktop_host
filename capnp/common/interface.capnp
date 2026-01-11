@0xbe052309f4b5a2e3 ;

struct InterfaceId {
  id @0 : UInt64 ;
}

enum InterfaceCardinality {
  atMostOne @0 ;
  exactlyOne @1 ;
  atLeastOne @2 ;
  any @3 ;
}
