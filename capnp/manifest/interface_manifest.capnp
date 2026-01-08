@0xbe052309f4b5a2e6 ;

using Interface = import "../common/interface.capnp" ;
using Version = import "../common/version.capnp" ;

struct InterfaceManifest {
  id @0 : Interface.InterfaceId ;
  version @1 : Version.Version ;
  cardinality @2 : Interface.InterfaceCardinality ;
}
