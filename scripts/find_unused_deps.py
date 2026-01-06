#!/usr/bin/env python3
import os
import sys
import glob
import toml

def get_workspace_dependencies(root_path):
    cargo_path = os.path.join(root_path, "Cargo.toml")
    with open(cargo_path, 'r') as f:
        data = toml.load(f)
    
    workspace_deps = []
    if "workspace" in data and "dependencies" in data["workspace"]:
        workspace_deps = list(data["workspace"]["dependencies"].keys())
    
    return workspace_deps

def get_workspace_members(root_path):
    cargo_path = os.path.join(root_path, "Cargo.toml")
    with open(cargo_path, 'r') as f:
        data = toml.load(f)
    
    members = []
    if "workspace" in data and "members" in data["workspace"]:
        members = data["workspace"]["members"]
        
    return members

def get_local_path_dependencies(root_path):
    cargo_path = os.path.join(root_path, "Cargo.toml")
    with open(cargo_path, 'r') as f:
        data = toml.load(f)
        
    local_paths = []
    if "workspace" in data and "dependencies" in data["workspace"]:
        for dep_name, dep_info in data["workspace"]["dependencies"].items():
            if isinstance(dep_info, dict) and "path" in dep_info:
                local_paths.append(dep_info["path"])
                
    return local_paths

def get_crate_dependencies(crate_path):
    cargo_path = os.path.join(crate_path, "Cargo.toml")

    if not os.path.exists(cargo_path):
        return set()
        
    with open(cargo_path, 'r') as f:
        data = toml.load(f)

    deps = set()
    
    # Standard dependencies
    if "dependencies" in data:
        deps.update(data["dependencies"].keys())
    
    if "dev-dependencies" in data:
        deps.update(data["dev-dependencies"].keys())
        
    if "build-dependencies" in data:
        deps.update(data["build-dependencies"].keys())
        
    # Target specific dependencies
    if "target" in data:
        for target_cfg in data["target"].values():
            if "dependencies" in target_cfg:
                deps.update(target_cfg["dependencies"].keys())
            if "dev-dependencies" in target_cfg:
                deps.update(target_cfg["dev-dependencies"].keys())
            if "build-dependencies" in target_cfg:
                deps.update(target_cfg["build-dependencies"].keys())

    return deps

def main():
    root_path = os.getcwd()
    print(f"Checking workspace dependencies in {root_path}...")
    
    try:
        workspace_deps = get_workspace_dependencies(root_path)
    except Exception as e:
        print(f"Error reading root Cargo.toml: {e}")
        sys.exit(1)
        
    print(f"Found {len(workspace_deps)} workspace dependencies.")
    
    try:
        members = get_workspace_members(root_path)
    except Exception as e:
        print(f"Error reading workspace members: {e}")
        sys.exit(1)

    print(f"Found {len(members)} workspace members.")
    
    local_path_deps = get_local_path_dependencies(root_path)
    print(f"Found {len(local_path_deps)} local path dependencies.")
    
    # Combine members and local path dependencies
    all_paths_to_scan = set()
    
    for member in members:
        all_paths_to_scan.add(member)
        
    for local_path in local_path_deps:
        all_paths_to_scan.add(local_path)
    
    used_deps = set()
    
    for relative_path in all_paths_to_scan:
        member_path = os.path.join(root_path, relative_path)
        # glob is useful if members contains wildcards, though here we likely just have paths
        # but let's keep it robust
        expanded_paths = glob.glob(member_path)
        
        for path in expanded_paths:
            try:
                deps = get_crate_dependencies(path)
                used_deps.update(deps)
            except Exception as e:
                # Some local paths might point to folders without Cargo.toml if configured weirdly?
                # or maybe just not initialized
                pass
            
    unused = []
    for dep in workspace_deps:
        if dep not in used_deps:
            unused.append(dep)
            
    if unused:
        print("\nFound unused workspace dependencies:")
        for dep in sorted(unused):
            print(f"  - {dep}")
        sys.exit(1)
    else:
        print("\nAll workspace dependencies are used.")
        sys.exit(0)

if __name__ == "__main__":
    main()