import pydivert
import time
import threading
import traceback

class BandwidthLimiter:
    def __init__(self):
        self._stop_event = threading.Event()
        self.download_limit_bps = 0  # 0 means unlimited
        self.upload_limit_bps = 0    # 0 means unlimited
        
        # Token bucket state
        self.bucket_in = 0.0
        self.bucket_out = 0.0
        self.last_check_in = time.time()
        self.last_check_out = time.time()
        
        self._thread = None
        self._windivert_handle = None  # Store handle for external close
        self._handle_lock = threading.Lock()
        
    def set_limits(self, download_mbps, upload_mbps):
        """Set limits in Mbps (Megabits per second)"""
        # Convert Mbps to bytes per second for easier handling with packet lengths
        # 1 Mbps = 1,000,000 bits/s = 125,000 bytes/s
        self.download_limit_bps = float(download_mbps) * 125000
        self.upload_limit_bps = float(upload_mbps) * 125000
        
        # Reset buckets to burst size (1 second worth of data or min 15KB)
        self.bucket_in = self.download_limit_bps if self.download_limit_bps > 0 else float('inf')
        self.bucket_out = self.upload_limit_bps if self.upload_limit_bps > 0 else float('inf')

    def _refill_bucket(self, current_bucket, limit_rate, last_check):
        now = time.time()
        elapsed = now - last_check
        if limit_rate <= 0:
            return float('inf'), now
        
        added = elapsed * limit_rate
        new_bucket = min(current_bucket + added, limit_rate * 1.0) # Max burst 1 second
        return new_bucket, now

    def start(self):
        self._stop_event.clear()
        self._thread = threading.Thread(target=self._worker)
        self._thread.daemon = False  # Non-daemon so cleanup runs properly
        self._thread.start()

    def stop(self):
        self._stop_event.set()
        # Close the WinDivert handle to interrupt blocking recv()
        with self._handle_lock:
            if self._windivert_handle is not None:
                try:
                    self._windivert_handle.close()
                except Exception:
                    pass
                self._windivert_handle = None
        # Wait for worker thread to finish
        if self._thread is not None:
            self._thread.join(timeout=5)
            self._thread = None
    
    def _worker(self):
        try:
            w = pydivert.WinDivert(filter="true")
            w.open()
            with self._handle_lock:
                self._windivert_handle = w
            
            try:
                while not self._stop_event.is_set():
                    try:
                        packet = w.recv(bufsize=65535)
                    except Exception as e:
                        if self._stop_event.is_set():
                            break
                        if "87" in str(e): 
                            print("WinDivert recv error 87 (packet too big?)")
                            continue
                        raise e

                    if self._stop_event.is_set():
                        try:
                            w.send(packet)
                        except Exception:
                            pass
                        break

                    packet_len = len(packet.raw)
                    is_outbound = packet.is_outbound
                    
                    limit = self.upload_limit_bps if is_outbound else self.download_limit_bps
                    
                    # If unlimited, just pass
                    if limit <= 0:
                        w.send(packet)
                        continue

                    # Token Bucket Logic
                    if is_outbound:
                        self.bucket_out, self.last_check_out = self._refill_bucket(
                            self.bucket_out, self.upload_limit_bps, self.last_check_out
                        )
                        bucket = self.bucket_out
                    else:
                        self.bucket_in, self.last_check_in = self._refill_bucket(
                            self.bucket_in, self.download_limit_bps, self.last_check_in
                        )
                        bucket = self.bucket_in
                    
                    if bucket >= packet_len:
                        # Enough tokens, pass
                        if is_outbound:
                            self.bucket_out -= packet_len
                        else:
                            self.bucket_in -= packet_len
                        w.send(packet)
                    else:
                        shortage = packet_len - bucket
                        wait_time = shortage / limit
                        
                        if wait_time > 0.5:
                            time.sleep(0.1)
                            if is_outbound:
                                self.bucket_out -= packet_len
                            else:
                                self.bucket_in -= packet_len
                        else:
                            time.sleep(wait_time)
                            if is_outbound:
                                self.bucket_out -= packet_len
                            else:
                                self.bucket_in -= packet_len
                        
                        w.send(packet)
            finally:
                # Always close the WinDivert handle to remove the kernel filter
                with self._handle_lock:
                    self._windivert_handle = None
                try:
                    w.close()
                except Exception:
                    pass

        except Exception as e:
            traceback.print_exc()
            print(f"Error in traffic shaper: {e}")
