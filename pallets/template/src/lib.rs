#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{
		pallet_prelude::*,
		traits::{tokens::ExistenceRequirement, Currency, Randomness},
	};
	use frame_system::pallet_prelude::*;
	use sp_io::hashing::blake2_128;
	use sp_runtime::ArithmeticError;
	use scale_info::{prelude::vec::Vec, TypeInfo};

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct NonFungibleToken<T: Config> {
		pub id: [u8; 16],
		pub name: Vec<u8>,
		pub url: Vec<u8>,
		pub price: Option<BalanceOf<T>>,
		pub owner: T::AccountId
	}

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	
		type Currency: Currency<Self::AccountId>;

		#[pallet::constant]
		type MaxNFTOwned: Get<u32>;

		type NFTRandomness: Randomness<Self::Hash, Self::BlockNumber>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	pub(super) type NFTCnt<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub(super) type NFTs<T: Config> = StorageMap<_, Twox64Concat, [u8; 16], NonFungibleToken<T>>;

	#[pallet::storage]
	pub(super) type NFTsOwned<T: Config> = StorageMap<
		_, 
		Twox64Concat, 
		T::AccountId, 
		BoundedVec<[u8; 16], T::MaxNFTOwned>, 
		ValueQuery,
	>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created { nft: [u8; 16], owner: T::AccountId },

		Sold { seller: T::AccountId, buyer: T::AccountId, nft: [u8; 16], price: BalanceOf<T> },
	
		Transfered { from: T::AccountId, to: T::AccountId, nft: [u8; 16] },
	
		PriceSet { nft: [u8; 16], price: Option<BalanceOf<T>> },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		TooManyOwned,
		NoNFT,
		NotOwner,
		BuyYourOwnNFT,
		DuplicateNFT,
		NotForSale
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn create_nft(origin: OriginFor<T>, name: Vec<u8>,url: Vec<u8>, price: Option<BalanceOf<T>>) -> DispatchResult {
			let owner = ensure_signed(origin)?;

			let random = T::NFTRandomness::random(&b"dna"[..]).0;

			let unique_payload = (
				random,
				frame_system::Pallet::<T>::extrinsic_index().unwrap_or_default(),
				frame_system::Pallet::<T>::block_number(),
			);

			let encoded_payload = unique_payload.encode();
			let hash = blake2_128(&encoded_payload);

			let nft = NonFungibleToken::<T> { id: hash, name, url, price, owner: owner.clone() };

			ensure!(!NFTs::<T>::contains_key(&nft.id), Error::<T>::DuplicateNFT);

			let cnt = NFTCnt::<T>::get();
			let new_cnt = cnt.checked_add(1).ok_or(ArithmeticError::Overflow)?;

			NFTsOwned::<T>::try_append(&owner, nft.id).map_err(|()| Error::<T>::TooManyOwned)?;

			NFTs::<T>::insert(nft.id, nft);
			NFTCnt::<T>::put(new_cnt);

			// Emit an event.
			Self::deposit_event(Event::Created{ nft: hash, owner });
			// Return a successful DispatchResultWithPostInfo
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn buy_nft(origin: OriginFor<T>, nft_id: [u8; 16]) -> DispatchResult {
			let buyer = ensure_signed(origin)?;

			let mut nft = NFTs::<T>::get(&nft_id).ok_or(Error::<T>::NoNFT)?;
 
			let from = nft.owner.clone(); 

			ensure!(nft.owner != buyer, Error::<T>::BuyYourOwnNFT);
			let mut from_owned = NFTsOwned::<T>::get(&from);

			if let Some(idx) = from_owned.iter().position(|&id| id == nft_id) {
				from_owned.swap_remove(idx);
			} else {
				return Err(Error::<T>::NoNFT.into())
			}

			let mut to_owned = NFTsOwned::<T>::get(&buyer);
			to_owned.try_push(nft_id).map_err(|()| Error::<T>::TooManyOwned)?;

			if let Some(price) = nft.price {
				T::Currency::transfer(&buyer, &from, price, ExistenceRequirement::KeepAlive)?;

				Self::deposit_event(Event::Sold {
					seller: nft.owner.clone(),
					buyer: buyer.clone(),
					nft: nft_id,
					price,
				});
			} else {
				return Err(Error::<T>::NotForSale.into())
			}

			nft.owner = buyer.clone();

			NFTs::<T>::insert(&nft_id, nft);
			NFTsOwned::<T>::insert(buyer.clone(), to_owned);
			NFTsOwned::<T>::insert(&from, from_owned);

			Self::deposit_event(Event::Transfered { from, to: buyer, nft: nft_id });

			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn set_price(origin: OriginFor<T>, nft_id: [u8; 16], price: Option<BalanceOf<T>>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			let mut nft = NFTs::<T>::get(&nft_id).ok_or(Error::<T>::NoNFT)?;
			ensure!(sender != nft.owner, Error::<T>::NotOwner);

			nft.price = price;
			NFTs::<T>::insert(&nft_id, nft);

			Self::deposit_event(Event::PriceSet { nft: nft_id, price });

			Ok(())
		}
	}
}
