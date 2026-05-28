#!/usr/bin/env python3
import subprocess
import sys
import os

def run_command(cmd, cwd=None):
    print(f"\n==========================================")
    print(f"Running: {' '.join(cmd)}")
    print(f"==========================================\n")
    try:
        res = subprocess.run(cmd, cwd=cwd, check=True)
        return res.returncode == 0
    except subprocess.CalledProcessError as e:
        print(f"\n❌ Error: Command failed with code {e.returncode}")
        return False
    except FileNotFoundError:
        print(f"\n❌ Error: Executable not found for {' '.join(cmd)}")
        return False

def main():
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    
    # 1. Rust backend tests
    rust_ok = run_command(["cargo", "test"], cwd=root_dir)
    
    # 2. Svelte frontend tests
    svelte_ok = run_command(["npm", "run", "test:unit"], cwd=root_dir)
    
    # 3. Pytest integration tests (if pytest is installed, else skip with warning)
    pytest_installed = False
    try:
        subprocess.run(["pytest", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        pytest_installed = True
    except (FileNotFoundError, PermissionError):
        pass
        
    if pytest_installed:
        integration_ok = run_command(["pytest", "tests/integration/"], cwd=root_dir)
    else:
        print("\n⚠️ Pytest not found. Skipping integration tests. Install with: pip install pytest")
        integration_ok = True
        
    if rust_ok and svelte_ok and integration_ok:
        print("\n🟢 All test suites passed successfully!")
        sys.exit(0)
    else:
        print("\n🔴 One or more test suites failed.")
        sys.exit(1)

if __name__ == "__main__":
    main()
