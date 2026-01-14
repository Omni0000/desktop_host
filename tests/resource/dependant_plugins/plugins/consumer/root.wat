(component
  ;; Import the resource interface from the counter plugin
  ;; When calling across plugin boundaries, results are wrapped in result<T, error>
  ;; Using unit for error type to simplify (we're ignoring errors for this test)
  (import "test:myresource/root" (instance $resource_inst
    (export "counter" (type $counter (sub resource)))
    (export "[constructor]counter" (func (result (result (own $counter)))))
    (export "[method]counter.get-value" (func (param "self" (borrow $counter)) (result (result u32))))
  ))

  ;; Alias the imported types and functions
  (alias export $resource_inst "counter" (type $counter))
  (alias export $resource_inst "[constructor]counter" (func $ctor_wrapped))
  (alias export $resource_inst "[method]counter.get-value" (func $get_wrapped))

  ;; Main core module - defines everything including memory
  (core module $main
    ;; Placeholders for imports - will be filled by lowered functions
    (import "resource" "ctor" (func $ctor (param i32)))
    (import "resource" "get" (func $get (param i32 i32)))

    (memory (export "memory") 1)

    ;; Realloc for canonical ABI
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 256  ;; Simple bump allocator
    )

    ;; Our exported get-value function
    (func (export "get-value") (result i32)
      ;; Call constructor with retptr = 0
      ;; Memory layout at 0: discriminant (i32), handle/payload (i32)
      i32.const 0
      call $ctor
      
      ;; Load handle from offset 4 (discriminant at 0 should be 0 for ok)
      ;; Call get-value with handle and retptr = 8
      (call $get
        (i32.load (i32.const 4))  ;; handle
        (i32.const 8)             ;; retptr for result
      )
      
      ;; Load result from offset 8: discriminant at 8, value at 12
      ;; Return the value at offset 12
      (i32.load (i32.const 12))
    )
  )

  ;; We need to instantiate the main module twice - first without imports to get
  ;; memory and realloc, then lower functions using those, then instantiate for real.
  ;; But we can't do that directly. Instead, use a two-phase approach with a separate
  ;; memory provider.

  ;; Alternative: Use the core module directly but with a trick - define memory
  ;; in a separate module, import it into main, and use it for lowering.

  ;; Let's try a different approach: define everything in components

  ;; Actually, the standard pattern is to have the main module export memory/realloc,
  ;; instantiate it with stubs, get the exports, then use them for lowering,
  ;; then reinstantiate properly.

  ;; But WAT component model doesn't allow re-instantiation. We need to use
  ;; a single instantiation with proper memory sharing.

  ;; The trick: have one module that just provides memory, import that memory
  ;; into the main module.

  ;; Memory provider module
  (core module $mem_module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 256
    )
  )
  (core instance $mem_inst (instantiate $mem_module))
  (alias core export $mem_inst "memory" (core memory $shared_mem))
  (alias core export $mem_inst "realloc" (core func $shared_realloc))

  ;; Lower the imported functions using shared memory
  (core func $lowered_ctor (canon lower (func $ctor_wrapped) (memory $shared_mem) (realloc $shared_realloc)))
  (core func $lowered_get (canon lower (func $get_wrapped) (memory $shared_mem) (realloc $shared_realloc)))

  ;; Create instance for imports
  (core instance $resource_imports
    (export "ctor" (func $lowered_ctor))
    (export "get" (func $lowered_get))
  )

  ;; Main module that imports the shared memory
  (core module $main_impl
    (import "resource" "ctor" (func $ctor (param i32)))
    (import "resource" "get" (func $get (param i32 i32)))
    (import "mem" "memory" (memory 1))

    ;; Our exported get-value function
    (func (export "get-value") (result i32)
      ;; Call constructor with retptr = 0
      i32.const 0
      call $ctor
      
      ;; Load handle from offset 4
      ;; Call get-value with handle and retptr = 8
      (call $get
        (i32.load (i32.const 4))
        (i32.const 8)
      )
      
      ;; Return the value at offset 12
      (i32.load (i32.const 12))
    )
  )

  ;; Memory imports instance
  (core instance $mem_imports
    (export "memory" (memory $shared_mem))
  )

  ;; Instantiate main module with proper imports
  (core instance $main_inst (instantiate $main_impl
    (with "resource" (instance $resource_imports))
    (with "mem" (instance $mem_imports))
  ))

  ;; Alias core export
  (alias core export $main_inst "get-value" (core func $core_get_value))

  ;; Lift the get-value function
  (func $lifted_get_value (result u32)
    (canon lift (core func $core_get_value))
  )

  ;; Export the consumer interface
  (instance $consumer_inst
    (export "get-value" (func $lifted_get_value))
  )
  (export "test:consumer/root" (instance $consumer_inst))
)
