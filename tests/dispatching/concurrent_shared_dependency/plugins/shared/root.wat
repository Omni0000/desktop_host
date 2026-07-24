(component
	(type $gate-interface (instance
		(type $wait (func async (result u32)))
		(export "wait" (func (type $wait)))
	))
	(import "test:gate/root" (instance $gate (type $gate-interface)))
	(alias export $gate "wait" (func $wait))
	(core module $memory
		(memory (export "memory") 1)
	)
	(core instance $memory (instantiate $memory))
	(core func $lowered_wait (canon lower (func $wait) async
		(memory $memory "memory")
	))
	(core func $task_return (canon task.return (result u32)))
	(core func $subtask_drop (canon subtask.drop))
	(core func $waitable_join (canon waitable.join))
	(core func $waitable_set_new (canon waitable-set.new))
	(core func $waitable_set_drop (canon waitable-set.drop))
	(core instance $imports
		(export "memory" (memory $memory "memory"))
		(export "wait" (func $lowered_wait))
		(export "task-return" (func $task_return))
		(export "subtask-drop" (func $subtask_drop))
		(export "waitable-join" (func $waitable_join))
		(export "waitable-set-new" (func $waitable_set_new))
		(export "waitable-set-drop" (func $waitable_set_drop))
	)

	(core module $m
		(import "runtime" "memory" (memory 1))
		(import "runtime" "wait" (func $wait (param i32) (result i32)))
		(import "runtime" "task-return" (func $task_return (param i32)))
		(import "runtime" "subtask-drop" (func $subtask_drop (param i32)))
		(import "runtime" "waitable-join" (func $waitable_join (param i32 i32)))
		(import "runtime" "waitable-set-new" (func $waitable_set_new (result i32)))
		(import "runtime" "waitable-set-drop" (func $waitable_set_drop (param i32)))

		(global $next_result (mut i32) (i32.const 4096))

		(func (export "wait") (result i32)
			(local $call i32)
			(local $result i32)
			(local $set i32)
			(local $subtask i32)

			global.get $next_result
			local.tee $result
			i32.const 4
			i32.add
			global.set $next_result

			local.get $result
			call $wait
			local.set $call
			local.get $call
			i32.const 15
			i32.and
			i32.const 2
			i32.eq
			if
				local.get $result
				i32.load
				call $task_return
				i32.const 0
				return
			end
			local.get $call
			i32.const 15
			i32.and
			i32.const 1
			i32.ne
			if
				unreachable
			end

			local.get $call
			i32.const 4
			i32.shr_u
			local.set $subtask
			call $waitable_set_new
			local.set $set

			local.get $subtask
			i32.const 8
			i32.mul
			i32.const 1024
			i32.add
			local.get $result
			i32.store
			local.get $subtask
			i32.const 8
			i32.mul
			i32.const 1028
			i32.add
			local.get $set
			i32.store

			local.get $subtask
			local.get $set
			call $waitable_join

			local.get $set
			i32.const 4
			i32.shl
			i32.const 2
			i32.or
		)

		(func (export "callback") (param $event i32) (param $handle i32) (param $status i32) (result i32)
			local.get $event
			i32.const 6
			i32.eq
			if
				i32.const 0
				return
			end
			local.get $event
			i32.const 1
			i32.ne
			if
				unreachable
			end
			local.get $status
			i32.const 4
			i32.eq
			if
				i32.const 0
				return
			end
			local.get $status
			i32.const 2
			i32.ne
			if
				unreachable
			end

			local.get $handle
			i32.const 8
			i32.mul
			i32.const 1024
			i32.add
			i32.load
			i32.load
			call $task_return

			local.get $handle
			call $subtask_drop
			local.get $handle
			i32.const 8
			i32.mul
			i32.const 1028
			i32.add
			i32.load
			call $waitable_set_drop

			i32.const 0
		)
	)
	(core instance $i (instantiate $m
		(with "runtime" (instance $imports))
	))
	(func $f async (result u32) (canon lift (core func $i "wait")
		(memory $memory "memory")
		async
		(callback (func $i "callback"))
	))
	(instance $root (export "wait" (func $f)))
	(export "test:concurrent-shared/root" (instance $root))
)
