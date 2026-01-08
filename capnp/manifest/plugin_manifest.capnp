@0xbe052309f4b5a2e5 ;

using Interface = import "../common/interface.capnp" ;
using Plugin = import "../common/plugin.capnp" ;
using Version = import "../common/version.capnp" ;

struct PluginManifest {
  id @0 : Plugin.PluginId ;
  version @1 : Version.Version ;
  plug @2 : Interface.InterfaceId ;
  sockets @3 : List( Interface.InterfaceId );
  permissions @4 : List( Permission );
  # checksum @5 : Data ;
}

# Permissions
struct Permission {
  union {
    web : group {
      url @0 : Text ;
    }
    foo : group {
      foo @1 : Text ;
    }
    bar : group {
      bar @2 : Text ;
    }
  }
}
