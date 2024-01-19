--!nocheck
--!nolint UnknownGlobal

local file = readFile("Cargo.toml")
if file ~= nil then
    print("Cargo.toml found!")
    print("Contents:")
    print(file)
end
