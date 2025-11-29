@0xbe052309f4b5a2e5 ;

# Primitives
struct PluginId {
  id @0 : Text ;
}

struct InterfaceId {
  id @0 : Text ;
}

# Metadata

struct Version {
  major @0 : UInt16 ;
  minor @1 : UInt16 ;
  patch @2 : UInt16 ;
}

enum InterfaceCardinality {
  one @0 ;
  many @1 ;
  atMostOne @2 ;
  atLeastOne @3 ;
}

# Basic Types

struct Interface {
  id @0 : InterfaceId ;
  version @1 : Version ;
  cardinality @2 : InterfaceCardinality ;
}

struct PluginMetadata {
  id @0 : PluginId ;
  version @1 : Version ;
  plug @2 : InterfaceId ;
  sockets @3 : List( InterfaceId );
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
