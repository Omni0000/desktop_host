(component
  (core module $m
    (func $startup (export "startup"))
  )
  (core instance $i (instantiate $m))
  (func $f (export "startup") (canon lift (core func $i "startup")))
  (instance $inst
    (export "startup" (func $f))
  )
  (export "root:startup/root" (instance $inst))
)