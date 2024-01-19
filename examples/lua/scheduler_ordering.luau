--!nocheck
--!nolint UnknownGlobal

print(1)

-- Defer will run at the end of the resumption cycle, but without yielding
__runtime__defer(function()
	print(5)
end)

-- Spawn will instantly run up until the first yield, and must then be resumed manually ...
__runtime__spawn(function()
	print(2)
	coroutine.yield()
	print("unreachable")
end)

-- ... unless calling functions created using `lua.create_async_function(...)`,
-- which will resume their calling thread with their result automatically
__runtime__spawn(function()
	print(3)
	wait(1)
	print(6)
end)

print(4)