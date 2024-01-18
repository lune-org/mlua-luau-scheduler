for i = 1, 5 do
	print("iteration " .. tostring(i) .. " of 5")
	local thread = coroutine.running()

	local counter = 0
	for j = 1, 10_000 do
		spawn(function()
			wait(0.1 * math.random())
			counter += 1
			if counter == 10_000 then
				print("completed iteration " .. tostring(i) .. " of 5")
				spawn(thread)
			end
		end)
	end

	coroutine.yield()
end
