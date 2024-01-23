--!nocheck
--!nolint UnknownGlobal


if math.random() < 0.25 then
	error("Unlucky error!")
end


local main = coroutine.running()
local start = os.clock()

local counter = 0
for j = 1, 10_000 do
	__runtime__spawn(function()
		sleep()
		counter += 1
		if counter == 10_000 then
			local elapsed = os.clock() - start
			__runtime__spawn(main, elapsed)
		end
	end)
end

return coroutine.yield()
