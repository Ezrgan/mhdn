use std::time::Duration;

use mhdn_rpc::fake_server::{FakeProcess, FakeRpcServer};
use mhdn_rpc::RpcError;
use mhdn_rpc::{ReadReq, RpcClient};

#[test]
fn read_memory_via_fake_server() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        st.memory.insert(0x1000, vec![0x07, 0x00, 0x00, 0xEB]);
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    let mut buf = [0u8; 4];
    client.read(0x1000, &mut buf).unwrap();
    assert_eq!(buf, [0x07, 0x00, 0x00, 0xEB]);
    server.shutdown();
}

#[test]
fn process_list_and_select() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        st.processes.push(FakeProcess {
            pid: 42,
            title_id: 0x0004_0000_0019_7100,
            name: *b"MHXXJP  ",
        });
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    let list = client.list_processes().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pid, 42);
    client.select_process(42).unwrap();
    assert_eq!(client.selected_process().unwrap(), 42);
    server.shutdown();
}

#[test]
fn read_chunks_over_1kb() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        let mut block = vec![0u8; 1500];
        for (i, b) in block.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        st.memory.insert(0x2000, block);
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    let mut buf = vec![0u8; 1500];
    client.read(0x2000, &mut buf).unwrap();
    for (i, b) in buf.iter().enumerate() {
        assert_eq!(*b, (i % 256) as u8);
    }
    server.shutdown();
}

#[test]
fn timeout_on_dropped_replies() {
    let (server, state) = FakeRpcServer::bind();
    state.lock().unwrap().drop_replies = true;
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(50)).unwrap();
    let err = client.read_u32(0).unwrap_err();
    assert!(matches!(err, RpcError::Timeout(_)));
    server.shutdown();
}

#[test]
fn write_inside_process_image_is_readable_back() {
    let (server, _state) = FakeRpcServer::bind();
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    client
        .write(0x00D3_2000, &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    let mut buf = [0u8; 4];
    client.read(0x00D3_2000, &mut buf).unwrap();
    assert_eq!(buf, [0x11, 0x22, 0x33, 0x44]);
    server.shutdown();
}

#[test]
fn write_outside_azahar_whitelist_is_rejected() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        st.memory.insert(0x3000_0000, vec![1, 2, 3, 4]);
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    let err = client.write(0x3000_0000, &[9, 9, 9, 9]).unwrap_err();
    assert!(matches!(err, RpcError::WriteRejected { addr: 0x3000_0000 }));
    let mut buf = [0u8; 4];
    client.read(0x3000_0000, &mut buf).unwrap();
    assert_eq!(buf, [1, 2, 3, 4]);
    server.shutdown();
}

#[test]
fn write_chunks_across_the_packet_limit() {
    let (server, _state) = FakeRpcServer::bind();
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    let data: Vec<u8> = (0..2000).map(|i| (i % 251) as u8).collect();
    client.write(0x00BF_2D00, &data).unwrap();
    let mut got = vec![0u8; data.len()];
    client.read(0x00BF_2D00, &mut got).unwrap();
    assert_eq!(got, data);
    server.shutdown();
}

#[test]
#[ignore = "writes 4 bytes of free .bss in a running Azahar and restores them"]
fn live_bss_write_roundtrip() {
    let addr = "127.0.0.1:45987".parse().unwrap();
    let mut client = RpcClient::connect(addr, Duration::from_millis(500)).unwrap();
    let mut original = [0u8; 4];
    client.read(0x00D3_2000, &mut original).unwrap();
    client
        .write(0x00D3_2000, &[0xA5, 0x5A, 0xA5, 0x5A])
        .unwrap();
    client.write(0x00D3_2000, &original).unwrap();
}

/// Well past the client timeout. Windows can stretch `SO_RCVTIMEO` by hundreds of milliseconds.
const LATE_REPLY: Duration = Duration::from_millis(800);

#[test]
fn late_replies_after_a_timeout_do_not_desync_later_reads() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        st.memory.insert(0x3000, vec![1, 2, 3, 4]);
        st.latency = LATE_REPLY;
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(30)).unwrap();
    let mut buf = [0u8; 4];
    assert!(client.read(0x3000, &mut buf).is_err());
    state.lock().unwrap().latency = Duration::ZERO;
    std::thread::sleep(Duration::from_millis(250));
    for _ in 0..3 {
        client.read(0x3000, &mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);
    }

    state.lock().unwrap().latency = LATE_REPLY;
    let mut reqs = [ReadReq {
        addr: 0x3000,
        buf: &mut buf,
    }];
    assert!(client.read_many(&mut reqs).is_err());
    state.lock().unwrap().latency = Duration::ZERO;
    std::thread::sleep(Duration::from_millis(250));
    let mut again = [0u8; 4];
    let mut reqs = [ReadReq {
        addr: 0x3000,
        buf: &mut again,
    }];
    client.read_many(&mut reqs).unwrap();
    assert_eq!(again, [1, 2, 3, 4]);
    server.shutdown();
}

#[test]
fn pipelined_read_many_with_reorder() {
    let (server, state) = FakeRpcServer::bind();
    {
        let mut st = state.lock().unwrap();
        st.reorder_replies = true;
        st.memory.insert(0x3000, vec![1, 2, 3, 4]);
        st.memory.insert(0x4000, vec![5, 6, 7, 8]);
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    client.set_pipeline_window(2);
    let mut a = [0u8; 4];
    let mut b = [0u8; 4];
    let mut reqs = [
        ReadReq {
            addr: 0x3000,
            buf: &mut a,
        },
        ReadReq {
            addr: 0x4000,
            buf: &mut b,
        },
    ];
    client.read_many(&mut reqs).unwrap();
    assert_eq!(a, [1, 2, 3, 4]);
    assert_eq!(b, [5, 6, 7, 8]);
    server.shutdown();
}
