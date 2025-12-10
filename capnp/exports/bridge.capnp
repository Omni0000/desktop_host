@0xbe36110ea09f3e3e ;

using Common  = import "../common.capnp" ;

struct FunctionCallInstruction {
  socket @0 : Common.InterfaceId ;
  function @1 : Text ;
}

struct FunctionCallResult {
  result : union {
    success @0 : List( FunctionCallResponse );
    failure @1 : FunctionCallError ;
  }
}

struct FunctionCallResponse {
  response : union {
    result @0 : Data ;
    deadlock @1 : Void ;
    malformed @2 : Void ;
    exception @3 : Data ;
  }
}

enum FunctionCallError {
  invalidInstructionMemorySegment @0 ;
  invalidInstructionData @1 ;
  invalidSocket @2 ;
  invalidArgsMemorySegment @3 ;
}