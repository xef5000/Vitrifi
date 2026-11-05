// The host authenticates the actor and supplies the domain-service adapter.
export interface CouponRequest {
  actorId: string;
  cartId: string;
  code: string;
}

export type CouponResponse =
  | { accepted: true; totalCents: number; discountCents: number; couponCode: string }
  | { accepted: false; error: string };

export type DiscountService = (request: CouponRequest) => Promise<CouponResponse>;

export async function handleCoupon(
  request: CouponRequest,
  service: DiscountService,
): Promise<CouponResponse> {
  if (!request.actorId || !request.cartId || !request.code.trim()) {
    return { accepted: false, error: "Customer, cart, and coupon are required" };
  }
  return service({ ...request, code: request.code.trim() });
}
