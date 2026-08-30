import time, subprocess, sys, os

modes = [
    # mode, value, notches
    ("axis_only", "15.0", "1"),
    ("discrete_only", "15.0", "1"),
    ("both", "15.0", "1"),
    ("both_rev", "15.0", "1"),
    ("axis_only", "120.0", "1"),
    ("discrete_only", "120.0", "1"),
    ("both", "120.0", "1"),
    ("both_rev", "120.0", "1"),
]

# We will just read the log from the end.
last_pos = 0
with open("/tmp/gtk_scroll.log", "r") as f:
    f.seek(0, 2)
    last_pos = f.tell()

for mode, val, notches in modes:
    print(f"\n[{mode} val={val}]")
    
    subprocess.run(["cargo", "run", "--bin", "test_scroll", mode, val, notches], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    
    time.sleep(0.5)
    
    with open("/tmp/gtk_scroll.log", "r") as f:
        f.seek(last_pos)
        lines = f.readlines()
        last_pos = f.tell()
        
    events = [l.strip() for l in lines if l.strip()]
    if events:
        print("RECEIVED:")
        for e in events:
            print("  " + e)
    else:
        print("RECEIVED: Nothing")

