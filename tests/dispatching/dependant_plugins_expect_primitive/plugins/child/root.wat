(component
  (core module $m
    (func $get_value (export "get-value") (result i32)
      i32.const 42
    )
  )
  (core instance $i (instantiate $m))
  (func $f (export "get-value") (result u32) (canon lift (core func $i "get-value")))
  (instance $inst
    (export "get-value" (func $f))
  )
   (export "test:child/root" (instance $inst))
)
