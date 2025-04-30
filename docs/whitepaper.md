1inch Fusion Protocol for Solana chain
======================================

## ABSTRACT

1inch Fusion introduces a front-running-resistant swapping mechanism. This protocol leverages the atomic swap, featuring a variable exchange rate that declines over time, based on a Dutch auction model. This model engages a network of professional market makers who compete for users’ orders. Designed to simplify the trading experience, Fusion combines the familiarity of traditional swaps with the security and efficiency of advanced order execution strategies.

## 1inch Fusion overview

1inch Fusion is a token swap protocol where professional market makers, called resolvers, compete to execute users' orders.. This competition is driven by a Dutch auction, where the exchange rate of an order decreases over time from an initial desirable rate to a minimal acceptable return. Each resolver must decide whether to fill the order promptly or wait for potentially higher profits, risking the chance to lose current arbitrage opportunities to their competitors.

The price of a Fusion order is determined by auction rules. Users (referred to as ‘makers’ within the protocol) are guaranteed by the Limit Order Protocol to not receive a rate lower than the Dutch auction's price. Resolvers are given the discretion to select liquidity sources and strategies to minimize gas usage, maintaining their competitiveness.

In summary, the scheme introduces the following benefits:

* makers pay transaction fees only for order creation, regardless of whether the swap succeeds or fails.

* makers are protected from maximum extractable value (MEV) attacks.

* makers receive the best rates from competing resolvers, which can be even better than the current market price.

* orders involving substantial asset amounts achieve more profitable fills through partial executions from multiple resolvers.

## Fusion settlement

1inch Fusion is built upon the 1inch Limit Order Protocol to guarantee gas-efficient, secure execution of orders and increase overall decentralization. To perform a swap:

1. A user executes a transaction to create a limit order.

2. The order is registered on-chain and within the 1inch Network, and then broadcasted to subscribed resolvers.

3. An authorized resolver sends a transaction to settle the order.

The order settlement process is shown in figure 1 where the user is the ‘maker’ and the resolver is the ‘taker.’ 

1. To fill a Fusion order, a resolver executes a fill order transaction providing a maker amount to fill.

2. The Limit Order Protocol calculates the taking amounts based on the provided amount and the price of the Dutch auction at the current timestamp in the block where the transaction is executed.

3. The protocol then transfers the maker’s assets to the resolver.

4. The protocol transfers the applicable fees to the respected destinations.

5. At this step, the protocol transfers all taker assets from the resolver wallet to the maker.

<div style="text-align:center">

![Figure 1: Fusion creation and settlement processes](/docs/main-flow.png)
*Figure 1: Fusion creation and settlement processes*

</div>


## Price and Dutch Auction

### Fusion order

Each Fusion order is filled through a competitive Dutch auction. At the beginning of the Dutch auction within Fusion, key parameters are used to govern the construction of the auction:

**Auction start timestamp**  
It determines when the auction officially starts. Any authorized resolver can fill an order with the defined maximum exchange rate before the auction start timestamp.

**Auction start rate**

The maximum exchange rate at which a user's order can be filled. Before the Dutch auction process begins, resolvers may fill the order at this rate.

**Minimum return amount**  
This is the lowest threshold of the exchange rate acceptable to the user, below which the order will not be filled. It effectively sets the floor price in the Dutch auction.

**Decrease rate**  
A rate at which the order's exchange rate declines over time once the auction has started. This is a piecewise linear function, included in the order description.

Resolvers compete to fill an order as soon as it becomes profitable for them, otherwise they risk losing the profits to another resolver.

<div style="text-align:center">

![Figure 2: Dutch auction price curve example](/docs/price-curve.png)
*Figure 2: Dutch auction price curve example*

</div>

### Dutch auction

The Dutch auction in 1inch Fusion orders does not decrease linearly. The auction duration is divided into several segments, each having its own decrease speed, which allows for an increased potential outcome depending on market conditions.

To define the curve for an order, 1inch utilizes a grid approach. Specifically, instead of starting the exchange of X token for Y token at the market price, the auction commences at a market exchange rate of X/6, referred to as the SpotPrice.

This methodology involves dividing the outgoing amount into smaller segments and subsequently observing the price at each segment. The outgoing amount X is divided into six equal parts, resulting in the following price points: X/6, 2X/6, 3X/6, 4X/6, 5X/6, and 6X/6.

Throughout approximately two-thirds of the auction duration, the process involves a descending adjustment from the initial SpotPrice towards the prevailing market price. This approach, in combination with partial fills, offers more favorable prices and quicker order fulfillment.

### Partial fills

The partial fill functionality enables large swaps to be executed even more efficiently, at rates better than current network market, as different resolvers fill different parts of the order. The intent-based approach ensures that the user does not pay any gas fees even if their order expires.

At each point of time the order can become profitable for a resolver, and they can fill it at the price designated by the curve either completely or partially. In case of partial fill, the auction goes on until the fill becomes profitable for the same or another resolver.  

Figure 2 illustrates a swap of 5,436 WETH to USDC. Within 1 minute, several resolvers executed 11 partial fills, ranging from 8.5% to 30.1% of the total swap amount in each block. As a result, the user received 40,524 USDC more than if they had swapped the entire amount at once at the current market price.

## Resolvers

Resolvers are agents within the Fusion ecosystem, acting as decentralized executors. They are professional market participants who bring efficiency to the order fulfillment process by harnessing their capital and trading strategies and gas-efficient execution. Resolvers compete with one another in Fusion’s Dutch auctions to fill orders as soon as they become profitable – this prioritizes timely execution and maximizes the economic incentive for both the user and resolver.

To participate in resolving Fusion swaps, a resolver’s address must be authorized by going through KYC/KYB procedure.

## Fees

Fees within the structure of the 1inch Fusion Protocol fall within two distinct categories:

**Protocol fee**  
A protocol fee that may be charged to resolvers to support the maintenance and further development of the protocol. Upon successful resolution of an order, a predetermined fee is deducted from the taker amount and goes to the wallet specified in the order structure.

**Integrator fees**  
In addition to Protocol Fees, Fusion Mode allows integrators to charge fees. The fee is taken directly from the swap volume and sent to any chosen address. This fee is denominated in taker tokens. 

The Protocol fee is to incentivize the adoption of 1inch Fusion protocol by third-party integrators.

**Surplus fee**

The Surplus Fee applies to trades executed at a rate significantly higher than the current market rate. A portion of this excess value is allocated to the DAO to support protocol operations. And the remaining part of the excess goes to a user.

## CONCLUSION

To summarize, we have introduced 1inch Fusion, a protocol that brings a MEV-resistant swapping mechanism to the decentralized finance (DeFi) ecosystem based on users' signed intents. By integrating a Dutch auction model, 1inch Fusion offers a trading experience that combines efficiency, security, and decentralization.