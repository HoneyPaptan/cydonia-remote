package sh.cydonia.remote

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionTest {
  @Test
  fun tailnetAndLoopbackAddressesMayUsePlainHttp() {
    for (host in listOf("100.101.14.98", "100.64.0.1", "100.127.255.254", "127.0.0.1", "localhost", "simple.tail1234.ts.net")) {
      assertTrue(host, Connection.tailnet(host))
    }
  }

  @Test
  fun everythingElseMustUseHttps() {
    for (host in listOf("192.168.1.5", "100.63.255.255", "100.128.0.1", "10.0.0.2", "example.com", "evil.ts.net.example.com", "")) {
      assertFalse(host, Connection.tailnet(host))
    }
  }
}
