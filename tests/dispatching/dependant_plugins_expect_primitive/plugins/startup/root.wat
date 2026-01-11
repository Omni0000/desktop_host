(component
  (core module $m
    (func $get_dependant_value (export "get-primitive") (result i32)
      i32.const 42
    )
  )
  (core instance $i (instantiate $m))
  (func $f (export "get-primitive") (result u32) (canon lift (core func $i "get-primitive")))
  (instance $inst
    (export "get-primitive" (func $f))
  )
  (export "test:dependant-primitive/root" (instance $inst))
)