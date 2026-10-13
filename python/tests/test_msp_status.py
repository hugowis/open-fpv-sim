"""The MSP_STATUS reader's framing (m4b debt): a reply split across TCP reads is waited for, not indexed past."""
from msp_status import reply_payload


def frame(cmd, payload):
    body = bytes([len(payload), cmd]) + payload
    checksum = 0
    for b in body:
        checksum ^= b
    return b"$M>" + body + bytes([checksum])


def test_a_reply_split_anywhere_waits_for_its_last_byte():
    whole = frame(101, bytes(range(20)))
    for cut in range(len(whole)):
        assert reply_payload(whole[:cut], 101) is None, cut
    assert reply_payload(whole, 101) == bytes(range(20))


def test_replies_to_other_commands_are_skipped():
    assert reply_payload(frame(105, b"\x01\x02") + frame(101, b"\x07"), 101) == b"\x07"
    assert reply_payload(frame(105, b"\x01\x02"), 101) is None
