# A host worker supplies the event and audit client after a successful checkout.
class DiscountAudit
  def initialize(audit_client)
    @audit_client = audit_client
  end

  def perform(event)
    return false unless event.fetch("accepted", false)

    @audit_client.record(
      "cart_id" => event.fetch("cartId"),
      "coupon_code" => event.fetch("couponCode"),
      "discount_cents" => event.fetch("discountCents")
    )
    true
  end
end
