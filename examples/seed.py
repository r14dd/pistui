"""Push synthetic traces to Langfuse for testing pistui."""

import os
import time
from langfuse import Langfuse

langfuse = Langfuse(
    host=os.environ["LANGFUSE_HOST"],
    public_key=os.environ["LANGFUSE_PUBLIC_KEY"],
    secret_key=os.environ["LANGFUSE_SECRET_KEY"],
)


def trace_rag_chatbot():
    """A RAG chatbot that retrieves context then generates a response."""
    with langfuse.start_as_current_observation(
        name="rag-chatbot",
        as_type="span",
        input={"query": "What is the refund policy for premium plans?"},
        metadata={"tags": ["production", "chatbot"]},
    ):
        with langfuse.start_as_current_observation(
            name="retrieve-context",
            as_type="span",
            input={"query": "refund policy premium", "top_k": 5},
        ) as retrieval:
            time.sleep(0.05)
            retrieval.update(output={
                "documents": [
                    {"id": "doc-42", "text": "Premium plans can be refunded within 30 days..."},
                    {"id": "doc-17", "text": "Refund requests must be submitted via the billing portal..."},
                    {"id": "doc-89", "text": "Enterprise plans have custom refund terms..."},
                ],
                "count": 3,
            })

        with langfuse.start_as_current_observation(
            name="gpt-4o-response",
            as_type="generation",
            model="gpt-4o",
            input=[
                {"role": "system", "content": "Answer using the provided context only."},
                {"role": "user", "content": "What is the refund policy for premium plans?"},
            ],
            model_parameters={"temperature": 0.2, "max_tokens": 500},
        ) as gen:
            time.sleep(0.1)
            gen.update(
                output="Premium plans can be refunded within 30 days of purchase. Submit your request through the billing portal.",
                usage_details={"input": 450, "output": 85},
            )


def trace_agent_with_tools():
    """An agent that calls tools and makes multiple LLM calls."""
    with langfuse.start_as_current_observation(
        name="travel-agent",
        as_type="span",
        input={"message": "Book me a flight from NYC to London next Friday"},
        metadata={"tags": ["staging", "agent"]},
    ):
        with langfuse.start_as_current_observation(
            name="plan-actions",
            as_type="generation",
            model="gpt-4o-mini",
            input=[
                {"role": "system", "content": "You are a travel booking agent."},
                {"role": "user", "content": "Book me a flight from NYC to London next Friday"},
            ],
        ) as plan:
            time.sleep(0.05)
            plan.update(
                output='{"actions": ["search_flights", "check_prices", "book_flight"]}',
                usage_details={"input": 120, "output": 45},
            )

        with langfuse.start_as_current_observation(
            name="search-flights",
            as_type="span",
            input={"from": "NYC", "to": "LHR", "date": "2026-06-20"},
        ) as search:
            time.sleep(0.08)
            search.update(output={"flights": [
                {"id": "BA178", "price": 850, "departure": "09:00", "arrival": "21:30"},
                {"id": "VS4", "price": 920, "departure": "11:00", "arrival": "23:15"},
                {"id": "AA100", "price": 780, "departure": "19:00", "arrival": "07:30+1"},
            ]})

        with langfuse.start_as_current_observation(
            name="check-prices",
            as_type="span",
            input={"flight_ids": ["BA178", "VS4", "AA100"]},
        ) as pricing:
            time.sleep(0.03)
            pricing.update(output={"best": "AA100", "savings": 70})

        with langfuse.start_as_current_observation(
            name="compose-response",
            as_type="generation",
            model="gpt-4o",
            input=[
                {"role": "system", "content": "Summarize the booking options for the user."},
                {"role": "user", "content": "Flights found: BA178 $850, VS4 $920, AA100 $780"},
            ],
        ) as respond:
            time.sleep(0.07)
            respond.update(
                output="I found 3 flights. The best deal is AA100 departing at 7 PM for $780. Want me to book it?",
                usage_details={"input": 280, "output": 65},
            )


def trace_failing_pipeline():
    """A pipeline where one step fails."""
    with langfuse.start_as_current_observation(
        name="document-summarizer",
        as_type="span",
        input={"url": "https://example.com/long-article"},
        metadata={"tags": ["production", "pipeline"]},
    ):
        with langfuse.start_as_current_observation(
            name="fetch-document",
            as_type="span",
            input={"url": "https://example.com/long-article"},
        ) as fetch:
            time.sleep(0.04)
            fetch.update(output={"length": 12500, "truncated": False})

        with langfuse.start_as_current_observation(
            name="chunk-text",
            as_type="span",
            input={"max_tokens": 4000},
        ) as chunk:
            time.sleep(0.02)
            chunk.update(output={"chunks": 4, "sizes": [3800, 3900, 3600, 1200]})

        for i in range(4):
            if i == 2:
                with langfuse.start_as_current_observation(
                    name=f"summarize-chunk-{i}",
                    as_type="generation",
                    model="gpt-4o-mini",
                    input=[{"role": "user", "content": f"Summarize chunk {i}"}],
                    level="ERROR",
                    status_message="Rate limited by OpenAI — 429 Too Many Requests",
                ) as gen:
                    time.sleep(0.06)
                    gen.update(usage_details={"input": 3600, "output": 0})
            else:
                with langfuse.start_as_current_observation(
                    name=f"summarize-chunk-{i}",
                    as_type="generation",
                    model="gpt-4o-mini",
                    input=[{"role": "user", "content": f"Summarize chunk {i}"}],
                ) as gen:
                    time.sleep(0.06)
                    gen.update(
                        output=f"Summary of chunk {i}: key points extracted.",
                        usage_details={"input": 2500, "output": 150},
                    )


if __name__ == "__main__":
    print("pushing traces...")
    trace_rag_chatbot()
    trace_agent_with_tools()
    trace_failing_pipeline()
    langfuse.flush()
    print("done — 3 traces pushed")
