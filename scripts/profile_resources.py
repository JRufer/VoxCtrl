#!/usr/bin/env python3
import os
import sys
import time
import subprocess
import re

def get_voxctrl_pids():
    """Find all PIDs associated with voxctrl and its WebKit subprocesses."""
    pids = set()
    try:
        # Get all PIDs of the current user
        output = subprocess.check_output(["ps", "-u", os.getenv("USER", "root"), "-o", "pid,comm,args"], text=True)
        for line in output.strip().split("\n")[1:]:
            parts = line.strip().split(None, 2)
            if len(parts) >= 2:
                pid = int(parts[0])
                comm = parts[1]
                args = parts[2] if len(parts) > 2 else ""
                
                # Check for voxctrl main bin, web process, or config processes
                if "voxctrl" in comm or "voxctrl" in args or "WebKit" in comm or "WebProcess" in comm:
                    # Ignore the python profiler script itself
                    if "profile_resources.py" not in args:
                        pids.add(pid)
    except Exception as e:
        print(f"Error scanning processes: {e}")
    return list(pids)

def get_process_stats(pid):
    """Retrieve RSS memory and command name for a given PID from /proc."""
    try:
        # Read RSS memory from /proc/[pid]/statm
        # Statm structure: size resident shared text library data dirty
        # page size is usually 4096 bytes
        with open(f"/proc/{pid}/statm", "r") as f:
            fields = f.read().split()
            rss_pages = int(fields[1])
            rss_mb = (rss_pages * 4096) / (1024 * 1024)
            
        with open(f"/proc/{pid}/cmdline", "r") as f:
            cmdline = f.read().replace("\x00", " ").strip()
            if not cmdline:
                with open(f"/proc/{pid}/comm", "r") as fc:
                    cmdline = fc.read().strip()
                    
        # Simplify command name for readability
        display_name = cmdline
        if len(display_name) > 60:
            display_name = display_name[:57] + "..."
            
        return {"pid": pid, "rss_mb": rss_mb, "name": display_name}
    except FileNotFoundError:
        # Process ended between scan and stat
        return None
    except Exception:
        return None

def get_vram_usage():
    """Query nvidia-smi for VRAM usage of active process IDs on Linux."""
    vram_map = {}
    try:
        # nvidia-smi can return process info in CSV format
        output = subprocess.check_output(
            ["nvidia-smi", "--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"],
            text=True,
            stderr=subprocess.DEVNULL
        )
        for line in output.strip().split("\n"):
            if line.strip():
                parts = line.split(",")
                if len(parts) == 2:
                    pid = int(parts[0].strip())
                    vram_mb = float(parts[1].strip())
                    vram_map[pid] = vram_mb
    except Exception:
        # Either no GPU driver, no nvidia-smi, or no nvidia card active
        pass
    return vram_map

def main():
    once_mode = "--once" in sys.argv
    
    if not once_mode:
        print("\033[2J\033[H", end="") # Clear terminal
    
    if not once_mode:
        print("======================================================================")
        print("          🎙️  VoxCtrl Real-Time Resource Profiler (RAM & VRAM)")
        print("======================================================================")
        print("  Watching all VoxCtrl Rust backends & WebKitGTK frontend subprocesses...")
        print("  Press Ctrl+C to exit.\n")
    
    try:
        while True:
            pids = get_voxctrl_pids()
            vram_map = get_vram_usage()
            
            total_ram = 0.0
            total_vram = 0.0
            
            # Print table headers
            if not once_mode:
                sys.stdout.write("\033[H") # Jump cursor to top
            print("======================================================================")
            print("          🎙️  VoxCtrl Real-Time Resource Profiler (RAM & VRAM)")
            print("======================================================================")
            print(f"  Status: Active  |  Interval: 1.0s  |  CUDA Detected: {'Yes' if vram_map else 'No'}\n")
            print(f"{'PID':<8} | {'SYS RAM (RSS)':<15} | {'GPU VRAM':<12} | {'PROCESS COMMAND'}")
            print("-" * 75)
            
            stats_list = []
            for pid in pids:
                stats = get_process_stats(pid)
                if stats:
                    vram = vram_map.get(pid, 0.0)
                    stats["vram_mb"] = vram
                    stats_list.append(stats)
            
            # Sort by highest RAM consumption
            stats_list.sort(key=lambda s: s["rss_mb"], reverse=True)
            
            for stats in stats_list:
                total_ram += stats["rss_mb"]
                total_vram += stats["vram_mb"]
                
                vram_str = f"{stats['vram_mb']:.1f} MB" if stats["vram_mb"] > 0 else "0.0 MB"
                if stats["vram_mb"] > 0:
                    vram_str = f"\033[1;32m{vram_str}\033[0m" # Highlight green if using VRAM
                
                print(f"{stats['pid']:<8} | {stats['rss_mb']:>10.1f} MB | {vram_str:>12} | {stats['name']}")
                
            print("-" * 75)
            
            total_vram_str = f"{total_vram:.1f} MB"
            if total_vram > 0:
                total_vram_str = f"\033[1;32m{total_vram_str}\033[0m"
                
            print(f"{'TOTAL':<8} | \033[1;36m{total_ram:>10.1f} MB\033[0m | {total_vram_str:>12} | ({len(stats_list)} processes active)")
            print("======================================================================\n")
            
            if once_mode:
                break
                
            # Clear trailing lines if processes count goes down
            sys.stdout.write("\033[J")
            sys.stdout.flush()
            time.sleep(1.0)
            
    except KeyboardInterrupt:
        print("\n\nProfiling terminated. Goodbye!")
        sys.exit(0)

if __name__ == "__main__":
    main()
