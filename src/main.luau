for i = 1, 20 do
	print("iteration " .. tostring(i) .. " of 20")
	local thread = coroutine.running()

	local counter = 0
	for j = 1, 50_000 do
		spawn(function()
			wait()
			counter += 1
			if counter == 50_000 then
				print("completed iteration " .. tostring(i) .. " of 20")
				spawn(thread)
			end
		end)
	end

	coroutine.yield()
end
