package club.touchtech.s5code.kotlin.transport

import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RpcChunkDeliveryTest {
    @Test
    fun `pending chunk survives disconnect closing its stream`() = runTest {
        val channel = Channel<JsonElement>(Channel.RENDEZVOUS)
        val delivery = launch(start = CoroutineStart.UNDISPATCHED) {
            deliverRpcChunk(channel, listOf(JsonPrimitive("snapshot")))
        }
        // The chunk dispatcher has looked up the channel but is still waiting
        // for its receiver when reconnect tears the old stream down.
        channel.close(RpcTransportClosed("Socket closed"))
        // The stream's awaitClose cancels the channel after its pump stops.
        channel.cancel()
        delivery.join()
        assertTrue(delivery.isCompleted)
        assertTrue(!delivery.isCancelled)
    }

    @Test
    fun `queued chunks after stream exit are harmless for every close cause`() = runTest {
        for (cause in listOf(null, RpcTransportClosed("Disconnected"),
            RpcFailure(RpcFailureKind.Fail, "NotFound", "Thread removed"))) {
            val channel = Channel<JsonElement>(Channel.RENDEZVOUS)
            channel.close(cause)
            deliverRpcChunk(channel, listOf(JsonPrimitive("late")))
        }
    }

    @Test
    fun `live chunk delivery retains all values and backpressure`() = runTest {
        val channel = Channel<JsonElement>(Channel.RENDEZVOUS)
        val values = listOf(JsonPrimitive("first"), JsonPrimitive("second"))
        val delivery = launch { deliverRpcChunk(channel, values) }
        assertEquals(values[0], channel.receive())
        assertEquals(values[1], channel.receive())
        delivery.join()
        channel.close()
    }

    @Test
    fun `cancelling a blocked delivery still cancels the coroutine`() = runTest {
        val channel = Channel<JsonElement>(Channel.RENDEZVOUS)
        val delivery = launch(start = CoroutineStart.UNDISPATCHED) {
            deliverRpcChunk(channel, listOf(JsonPrimitive("pending")))
        }
        delivery.cancel()
        delivery.join()
        assertTrue(delivery.isCancelled)
        channel.close()
    }
}
