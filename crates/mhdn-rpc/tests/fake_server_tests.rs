use std::sync::{Arc, Mutex};
use std::time::Duration;

use mhdn_rpc::fake_server::{FakeProcess, FakeRpcServer, ServerState};
use mhdn_rpc::RpcError;
use mhdn_rpc::{ReadReq, RpcClient, RpcProtocol};

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
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(50)).unwrap();
    state.lock().unwrap().drop_replies = true;
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
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(30)).unwrap();
    state.lock().unwrap().latency = LATE_REPLY;
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
        st.memory.insert(0x3000, vec![1, 2, 3, 4]);
        st.memory.insert(0x4000, vec![5, 6, 7, 8]);
    }
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    state.lock().unwrap().reorder_replies = true;
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

fn connect_dialect(protocol: RpcProtocol) -> (FakeRpcServer, Arc<Mutex<ServerState>>, RpcClient) {
    let (server, state) = FakeRpcServer::bind_protocol(protocol);
    {
        let mut st = state.lock().unwrap();
        st.processes.push(FakeProcess {
            pid: 42,
            title_id: 0x0004_0000_0019_7100,
            name: *b"MHXXJP  ",
        });
        st.memory.insert(0x1000, vec![0x07, 0x00, 0x00, 0xEB]);
        let mut block = vec![0u8; 1500];
        for (i, byte) in block.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }
        st.memory.insert(0x2000, block);
    }
    let client = RpcClient::connect(server.addr(), Duration::from_millis(500)).unwrap();
    (server, state, client)
}

fn assert_list_select_and_read(client: &mut RpcClient) {
    let list = client.list_processes().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pid, 42);
    assert_eq!(list[0].title_id, 0x0004_0000_0019_7100);
    client.select_process(42).unwrap();
    assert_eq!(client.selected_process().unwrap(), 42);
    let mut buf = [0u8; 4];
    client.read(0x1000, &mut buf).unwrap();
    assert_eq!(buf, [0x07, 0x00, 0x00, 0xEB]);
}

#[test]
fn v1_server_is_not_mistaken_for_2126_2() {
    let (server, state, mut client) = connect_dialect(RpcProtocol::V1);
    assert_eq!(client.protocol(), RpcProtocol::V1);
    assert_list_select_and_read(&mut client);
    state.lock().unwrap().read_requests = 0;
    let mut big = vec![0u8; 1500];
    client.read(0x2000, &mut big).unwrap();
    assert_eq!(state.lock().unwrap().read_requests, 2);
    for (i, byte) in big.iter().enumerate() {
        assert_eq!(*byte, (i % 256) as u8);
    }
    server.shutdown();
}

#[test]
fn v2_server_answers_2126_2_process_list_and_reads() {
    let (server, state, mut client) = connect_dialect(RpcProtocol::V2);
    assert_eq!(client.protocol(), RpcProtocol::V2);
    assert_eq!(client.protocol().max_packet_data_size, 32 * 1024);
    assert_list_select_and_read(&mut client);
    state.lock().unwrap().read_requests = 0;
    let mut big = vec![0u8; 1500];
    client.read(0x2000, &mut big).unwrap();
    assert_eq!(state.lock().unwrap().read_requests, 1);
    for (i, byte) in big.iter().enumerate() {
        assert_eq!(*byte, (i % 256) as u8);
    }
    server.shutdown();
}

#[test]
fn a_server_that_rejects_every_probed_version_does_not_connect() {
    let (server, state) = FakeRpcServer::bind_protocol(RpcProtocol::V2);
    state.lock().unwrap().reject_all = true;
    let Err(err) = RpcClient::connect(server.addr(), Duration::from_millis(300)) else {
        panic!("connect succeeded against a server that rejects every version");
    };
    assert!(matches!(err, RpcError::InvalidResponse));
    // Version 2, then 1, then 3..=16. An echoed empty body is not a process list.
    assert_eq!(state.lock().unwrap().requests, 16);
    server.shutdown();
}

fn one_game(protocol: RpcProtocol) -> (FakeRpcServer, Arc<Mutex<ServerState>>) {
    let (server, state) = FakeRpcServer::bind_protocol(protocol);
    state.lock().unwrap().processes.push(FakeProcess {
        pid: 42,
        title_id: 0x0004_0000_0019_7100,
        name: *b"MHXXJP  ",
    });
    (server, state)
}

#[test]
fn version_2_is_accepted_without_probing_further() {
    let (server, state) = one_game(RpcProtocol::V2);
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(200)).unwrap();
    assert_eq!(client.protocol(), RpcProtocol::V2);
    assert_eq!(state.lock().unwrap().requests, 1);
    assert_eq!(client.list_processes().unwrap().len(), 1);
    server.shutdown();
}

#[test]
fn version_1_is_chosen_after_an_empty_v2_rejection() {
    let (server, state) = one_game(RpcProtocol::V1);
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(200)).unwrap();
    assert_eq!(client.protocol(), RpcProtocol::V1);
    // Version 2 echoed back empty, then version 1 returned a list. 3..=16 stay untried.
    assert_eq!(state.lock().unwrap().requests, 2);
    assert_eq!(client.list_processes().unwrap().len(), 1);
    server.shutdown();
}

#[test]
fn an_announced_header_version_is_retried_once() {
    let protocol = RpcProtocol {
        version: 7,
        max_packet_data_size: mhdn_rpc::MAX_PACKET_DATA_SIZE,
    };
    let (server, state) = one_game(protocol);
    state.lock().unwrap().announce_version = Some(7);
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(200)).unwrap();
    assert_eq!(client.protocol().version, 7);
    assert_eq!(client.protocol().max_packet_data_size, 32 * 1024);
    // Version 2 rejected with header version 7, then one retry. No sweep.
    assert_eq!(state.lock().unwrap().requests, 2);
    assert_eq!(client.list_processes().unwrap().len(), 1);
    server.shutdown();
}

#[test]
fn version_5_is_found_by_sweep_and_stops_there() {
    let protocol = RpcProtocol {
        version: 5,
        max_packet_data_size: mhdn_rpc::MAX_PACKET_DATA_SIZE,
    };
    let (server, state) = one_game(protocol);
    let mut client = RpcClient::connect(server.addr(), Duration::from_millis(200)).unwrap();
    assert_eq!(client.protocol().version, 5);
    assert_eq!(client.protocol().max_packet_data_size, 32 * 1024);
    // 2 (echoed empty), 1, 3, 4, then 5 returns a list. 6..=16 stay untried.
    assert_eq!(state.lock().unwrap().requests, 5);
    assert_eq!(client.list_processes().unwrap().len(), 1);
    server.shutdown();
}

#[test]
fn a_silent_server_is_not_swept() {
    let (server, state) = FakeRpcServer::bind_protocol(RpcProtocol::V2);
    state.lock().unwrap().drop_replies = true;
    let Err(err) = RpcClient::connect(server.addr(), Duration::from_millis(50)) else {
        panic!("connect succeeded against a server that does not answer");
    };
    assert!(matches!(err, RpcError::Timeout(_)));
    assert_eq!(state.lock().unwrap().requests, 1);
    server.shutdown();
}
