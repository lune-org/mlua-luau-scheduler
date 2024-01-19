local start = os.clock()

local thread = coroutine.running()

local counter = 0
for j = 1, 10_000 do
	spawn(function()
		wait()
		counter += 1
		if counter == 10_000 then
			print("completed")
			spawn(thread)
		end
	end)
end

coroutine.yield()

return os.clock() - start
