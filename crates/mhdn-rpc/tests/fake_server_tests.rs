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
